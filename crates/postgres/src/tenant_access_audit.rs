use fleetiq_application::{
    TenantAccessAuditCursor, TenantAccessAuditEntry, TenantAccessAuditPage, TenantAccessAuditQuery,
    TenantAccessAuditRepository, TenantAccessChangeEffect, TenantAccessChangeKind, TenantAction,
};
use fleetiq_domain::{ActorId, TenantAccessChangeId, TenantId, UnixTimeMillis};
use sqlx::PgPool;

use crate::{PostgresRepositoryError, error::corrupt};

/// PostgreSQL repository for bounded tenant access audit queries.
#[derive(Clone, Debug)]
pub struct PostgresTenantAccessAuditRepository {
    pool: PgPool,
}

impl PostgresTenantAccessAuditRepository {
    /// Creates an audit repository using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl TenantAccessAuditRepository for PostgresTenantAccessAuditRepository {
    type Error = PostgresRepositoryError;

    async fn find_page(
        &self,
        tenant_id: &TenantId,
        query: &TenantAccessAuditQuery,
    ) -> Result<TenantAccessAuditPage, Self::Error> {
        let after_recorded_at = query.after().map(|cursor| cursor.recorded_at().get());
        let after_change_id = query.after().map(|cursor| cursor.change_id().as_str());
        let requested = i64::from(query.page_size().get());
        let fetch_limit = requested + 1;

        let rows = sqlx::query!(
            r#"
            SELECT change_id, tenant_id, administrator_actor_id, target_actor_id,
                   action, change_kind, recorded_at_ms, state_changed
            FROM identity.tenant_access_changes
            WHERE tenant_id = $1
              AND (
                    $2::bigint IS NULL
                    OR (recorded_at_ms, change_id) < ($2, $3)
                  )
            ORDER BY recorded_at_ms DESC, change_id DESC
            LIMIT $4
            "#,
            tenant_id.as_str(),
            after_recorded_at,
            after_change_id,
            fetch_limit,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresRepositoryError::Database)?;

        let mut entries = rows
            .into_iter()
            .map(|row| {
                Ok(TenantAccessAuditEntry::new(
                    TenantAccessChangeId::new(row.change_id).map_err(|error| {
                        corrupt("identity.tenant_access_changes.change_id", error)
                    })?,
                    TenantId::new(row.tenant_id).map_err(|error| {
                        corrupt("identity.tenant_access_changes.tenant_id", error)
                    })?,
                    ActorId::new(row.administrator_actor_id).map_err(|error| {
                        corrupt(
                            "identity.tenant_access_changes.administrator_actor_id",
                            error,
                        )
                    })?,
                    ActorId::new(row.target_actor_id).map_err(|error| {
                        corrupt("identity.tenant_access_changes.target_actor_id", error)
                    })?,
                    action(&row.action)?,
                    kind(&row.change_kind)?,
                    UnixTimeMillis::new(row.recorded_at_ms),
                    if row.state_changed {
                        TenantAccessChangeEffect::StateChanged
                    } else {
                        TenantAccessChangeEffect::NoStateChange
                    },
                ))
            })
            .collect::<Result<Vec<_>, PostgresRepositoryError>>()?;

        let has_more = entries.len() > usize::from(query.page_size().get());
        if has_more {
            entries.pop();
        }
        let next = has_more.then(|| {
            let last = entries
                .last()
                .expect("a page with an extra row retains at least one entry");
            TenantAccessAuditCursor::new(last.recorded_at(), last.change_id().clone())
        });

        Ok(TenantAccessAuditPage::new(entries, next))
    }
}

fn action(value: &str) -> Result<TenantAction, PostgresRepositoryError> {
    match value {
        "metadata.read" => Ok(TenantAction::ReadMetadata),
        "metadata.manage" => Ok(TenantAction::ManageMetadata),
        "access.manage" => Ok(TenantAction::ManageAccess),
        _ => Err(corrupt("identity.tenant_access_changes.action", value)),
    }
}

fn kind(value: &str) -> Result<TenantAccessChangeKind, PostgresRepositoryError> {
    match value {
        "grant" => Ok(TenantAccessChangeKind::Grant),
        "revoke" => Ok(TenantAccessChangeKind::Revoke),
        _ => Err(corrupt("identity.tenant_access_changes.change_kind", value)),
    }
}

/// Error returned by PostgreSQL tenant access audit queries.
pub type PostgresTenantAccessAuditRepositoryError = PostgresRepositoryError;

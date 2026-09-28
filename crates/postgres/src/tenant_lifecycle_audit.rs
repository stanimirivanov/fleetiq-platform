use fleetiq_application::{
    TenantLifecycleAuditCursor, TenantLifecycleAuditEntry, TenantLifecycleAuditPage,
    TenantLifecycleAuditQuery, TenantLifecycleAuditRepository, TenantLifecycleChangeEffect,
    TenantLifecycleChangeKind,
};
use fleetiq_domain::{ActorId, TenantId, TenantLifecycleChangeId, UnixTimeMillis};
use sqlx::PgPool;

use crate::{PostgresRepositoryError, error::corrupt};

/// PostgreSQL repository for bounded tenant lifecycle audit queries.
#[derive(Clone, Debug)]
pub struct PostgresTenantLifecycleAuditRepository {
    pool: PgPool,
}

impl PostgresTenantLifecycleAuditRepository {
    /// Creates an audit repository using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl TenantLifecycleAuditRepository for PostgresTenantLifecycleAuditRepository {
    type Error = PostgresRepositoryError;

    async fn find_page(
        &self,
        query: &TenantLifecycleAuditQuery,
    ) -> Result<TenantLifecycleAuditPage, Self::Error> {
        let after_recorded_at = query.after().map(|cursor| cursor.recorded_at().get());
        let after_change_id = query.after().map(|cursor| cursor.change_id().as_str());
        let requested = i64::from(query.page_size().get());
        let fetch_limit = requested + 1;

        let rows = sqlx::query!(
            r#"
            SELECT change_id, administrator_actor_id, tenant_id,
                   change_kind, recorded_at_ms, state_changed
            FROM identity.tenant_lifecycle_changes
            WHERE $1::bigint IS NULL
               OR (recorded_at_ms, change_id) < ($1, $2)
            ORDER BY recorded_at_ms DESC, change_id DESC
            LIMIT $3
            "#,
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
                Ok(TenantLifecycleAuditEntry::new(
                    TenantLifecycleChangeId::new(row.change_id).map_err(|error| {
                        corrupt("identity.tenant_lifecycle_changes.change_id", error)
                    })?,
                    ActorId::new(row.administrator_actor_id).map_err(|error| {
                        corrupt(
                            "identity.tenant_lifecycle_changes.administrator_actor_id",
                            error,
                        )
                    })?,
                    TenantId::new(row.tenant_id).map_err(|error| {
                        corrupt("identity.tenant_lifecycle_changes.tenant_id", error)
                    })?,
                    kind(&row.change_kind)?,
                    UnixTimeMillis::new(row.recorded_at_ms),
                    if row.state_changed {
                        TenantLifecycleChangeEffect::StateChanged
                    } else {
                        TenantLifecycleChangeEffect::NoStateChange
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
            TenantLifecycleAuditCursor::new(last.recorded_at(), last.change_id().clone())
        });

        Ok(TenantLifecycleAuditPage::new(entries, next))
    }
}

fn kind(value: &str) -> Result<TenantLifecycleChangeKind, PostgresRepositoryError> {
    match value {
        "activate" => Ok(TenantLifecycleChangeKind::Activate),
        "suspend" => Ok(TenantLifecycleChangeKind::Suspend),
        _ => Err(corrupt(
            "identity.tenant_lifecycle_changes.change_kind",
            value,
        )),
    }
}

/// Error returned by PostgreSQL tenant lifecycle audit queries.
pub type PostgresTenantLifecycleAuditRepositoryError = PostgresRepositoryError;

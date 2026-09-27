use fleetiq_application::{
    PlatformAccessAuditCursor, PlatformAccessAuditEntry, PlatformAccessAuditPage,
    PlatformAccessAuditQuery, PlatformAccessAuditRepository, PlatformAccessChangeEffect,
    PlatformAccessChangeKind, PlatformAction,
};
use fleetiq_domain::{ActorId, PlatformAccessChangeId, UnixTimeMillis};
use sqlx::PgPool;

use crate::{PostgresRepositoryError, error::corrupt};

/// PostgreSQL repository for bounded platform access audit queries.
#[derive(Clone, Debug)]
pub struct PostgresPlatformAccessAuditRepository {
    pool: PgPool,
}

impl PostgresPlatformAccessAuditRepository {
    /// Creates an audit repository using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl PlatformAccessAuditRepository for PostgresPlatformAccessAuditRepository {
    type Error = PostgresRepositoryError;

    async fn find_page(
        &self,
        query: &PlatformAccessAuditQuery,
    ) -> Result<PlatformAccessAuditPage, Self::Error> {
        let after_recorded_at = query.after().map(|cursor| cursor.recorded_at().get());
        let after_change_id = query.after().map(|cursor| cursor.change_id().as_str());
        let requested = i64::from(query.page_size().get());
        let fetch_limit = requested + 1;

        let rows = sqlx::query!(
            r#"
            SELECT change_id, administrator_actor_id, target_actor_id, action,
                   change_kind, recorded_at_ms, state_changed
            FROM identity.platform_access_changes
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
                Ok(PlatformAccessAuditEntry::new(
                    PlatformAccessChangeId::new(row.change_id).map_err(|error| {
                        corrupt("identity.platform_access_changes.change_id", error)
                    })?,
                    ActorId::new(row.administrator_actor_id).map_err(|error| {
                        corrupt(
                            "identity.platform_access_changes.administrator_actor_id",
                            error,
                        )
                    })?,
                    ActorId::new(row.target_actor_id).map_err(|error| {
                        corrupt("identity.platform_access_changes.target_actor_id", error)
                    })?,
                    action(&row.action)?,
                    kind(&row.change_kind)?,
                    UnixTimeMillis::new(row.recorded_at_ms),
                    if row.state_changed {
                        PlatformAccessChangeEffect::StateChanged
                    } else {
                        PlatformAccessChangeEffect::NoStateChange
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
            PlatformAccessAuditCursor::new(last.recorded_at(), last.change_id().clone())
        });

        Ok(PlatformAccessAuditPage::new(entries, next))
    }
}

fn action(value: &str) -> Result<PlatformAction, PostgresRepositoryError> {
    match value {
        "platform.access.manage" => Ok(PlatformAction::ManagePlatformAccess),
        "identity.manage" => Ok(PlatformAction::ManageIdentity),
        "tenant.provision" => Ok(PlatformAction::ProvisionTenant),
        _ => Err(corrupt("identity.platform_access_changes.action", value)),
    }
}

fn kind(value: &str) -> Result<PlatformAccessChangeKind, PostgresRepositoryError> {
    match value {
        "grant" => Ok(PlatformAccessChangeKind::Grant),
        "revoke" => Ok(PlatformAccessChangeKind::Revoke),
        _ => Err(corrupt(
            "identity.platform_access_changes.change_kind",
            value,
        )),
    }
}

/// Error returned by PostgreSQL platform access audit queries.
pub type PostgresPlatformAccessAuditRepositoryError = PostgresRepositoryError;

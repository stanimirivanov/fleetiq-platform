use std::{error::Error, fmt};

use crate::action_codec::tenant_action_name;
use fleetiq_domain::{TenantAccessChangeId, TenantId};
use fleetiq_identity::{
    TenantAccessAdministrationRepository, TenantAccessChange, TenantAccessChangeEffect,
    TenantAccessChangeKind, TenantAccessChangeOutcome,
};

use sqlx::PgPool;

use crate::{PostgresRepositoryError, error::classify_database_error};

/// PostgreSQL repository for atomic tenant grant changes and immutable audit.
#[derive(Clone, Debug)]
pub struct PostgresTenantAccessAdministration {
    pool: PgPool,
}

impl PostgresTenantAccessAdministration {
    /// Creates a repository using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl TenantAccessAdministrationRepository for PostgresTenantAccessAdministration {
    type Error = PostgresTenantAccessAdministrationError;

    /// Serializes changes per tenant, mutates current grants, and appends audit
    /// in one transaction. The stable tenant row is locked before checking the
    /// command identity, preventing concurrent uses of one tenant-scoped change
    /// ID from racing when they target different actors.
    async fn apply(
        &self,
        change: &TenantAccessChange,
    ) -> Result<TenantAccessChangeOutcome, Self::Error> {
        let mut transaction = self.pool.begin().await.map_err(repository_error)?;

        sqlx::query!(
            r#"
            SELECT tenant_id
            FROM metadata.tenants
            WHERE tenant_id = $1
            FOR UPDATE
            "#,
            change.tenant_id().as_str(),
        )
        .fetch_optional(&mut *transaction)
        .await
        .map_err(repository_error)?;

        if let Some(existing) = sqlx::query!(
            r#"
            SELECT administrator_actor_id, target_actor_id, action, change_kind,
                   recorded_at_ms, state_changed
            FROM identity.tenant_access_changes
            WHERE tenant_id = $1 AND change_id = $2
            "#,
            change.tenant_id().as_str(),
            change.change_id().as_str(),
        )
        .fetch_optional(&mut *transaction)
        .await
        .map_err(repository_error)?
        {
            if existing.administrator_actor_id != change.administrator_actor_id().as_str()
                || existing.target_actor_id != change.target_actor_id().as_str()
                || existing.action != tenant_action_name(change.action())
                || existing.change_kind != kind_name(change.kind())
                || existing.recorded_at_ms != change.recorded_at().get()
            {
                return Err(PostgresTenantAccessAdministrationError::IdentityCollision {
                    tenant_id: change.tenant_id().clone(),
                    change_id: change.change_id().clone(),
                });
            }

            transaction.commit().await.map_err(repository_error)?;
            return Ok(TenantAccessChangeOutcome::AlreadyRecorded(effect(
                existing.state_changed,
            )));
        }

        let result = match change.kind() {
            TenantAccessChangeKind::Grant => {
                sqlx::query!(
                    r#"
                    INSERT INTO identity.tenant_action_grants
                        (tenant_id, actor_id, action)
                    VALUES ($1, $2, $3)
                    ON CONFLICT DO NOTHING
                    "#,
                    change.tenant_id().as_str(),
                    change.target_actor_id().as_str(),
                    tenant_action_name(change.action()),
                )
                .execute(&mut *transaction)
                .await
            }
            TenantAccessChangeKind::Revoke => {
                sqlx::query!(
                    r#"
                    DELETE FROM identity.tenant_action_grants
                    WHERE tenant_id = $1 AND actor_id = $2 AND action = $3
                    "#,
                    change.tenant_id().as_str(),
                    change.target_actor_id().as_str(),
                    tenant_action_name(change.action()),
                )
                .execute(&mut *transaction)
                .await
            }
        }
        .map_err(repository_error)?;

        let state_changed = result.rows_affected() == 1;
        sqlx::query!(
            r#"
            INSERT INTO identity.tenant_access_changes
                (tenant_id, change_id, administrator_actor_id, target_actor_id,
                 action, change_kind, recorded_at_ms, state_changed)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            change.tenant_id().as_str(),
            change.change_id().as_str(),
            change.administrator_actor_id().as_str(),
            change.target_actor_id().as_str(),
            tenant_action_name(change.action()),
            kind_name(change.kind()),
            change.recorded_at().get(),
            state_changed,
        )
        .execute(&mut *transaction)
        .await
        .map_err(repository_error)?;

        transaction.commit().await.map_err(repository_error)?;
        Ok(TenantAccessChangeOutcome::Recorded(effect(state_changed)))
    }
}

const fn kind_name(kind: TenantAccessChangeKind) -> &'static str {
    match kind {
        TenantAccessChangeKind::Grant => "grant",
        TenantAccessChangeKind::Revoke => "revoke",
    }
}

const fn effect(state_changed: bool) -> TenantAccessChangeEffect {
    if state_changed {
        TenantAccessChangeEffect::StateChanged
    } else {
        TenantAccessChangeEffect::NoStateChange
    }
}

fn repository_error(error: sqlx::Error) -> PostgresTenantAccessAdministrationError {
    PostgresTenantAccessAdministrationError::Repository(classify_database_error(error))
}

/// Failure to persist or resolve a tenant access administration command.
#[derive(Debug)]
pub enum PostgresTenantAccessAdministrationError {
    /// A tenant-scoped command identity already names different content.
    IdentityCollision {
        /// Tenant that scopes the command identity.
        tenant_id: TenantId,
        /// Reused command identity.
        change_id: TenantAccessChangeId,
    },
    /// Lower-level PostgreSQL repository failure.
    Repository(PostgresRepositoryError),
}

impl fmt::Display for PostgresTenantAccessAdministrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityCollision {
                tenant_id,
                change_id,
            } => write!(
                formatter,
                "tenant access change identity collision for {tenant_id}/{change_id}"
            ),
            Self::Repository(_) => {
                formatter.write_str("PostgreSQL tenant access administration failed")
            }
        }
    }
}

impl Error for PostgresTenantAccessAdministrationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::IdentityCollision { .. } => None,
            Self::Repository(error) => Some(error),
        }
    }
}

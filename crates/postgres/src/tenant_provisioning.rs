use std::{error::Error, fmt};

use fleetiq_domain::{TenantId, TenantProvisioningId};
use fleetiq_identity::{
    TenantProvisioningCommand, TenantProvisioningOutcome, TenantProvisioningRepository,
};
use sqlx::PgPool;

use crate::{PostgresRepositoryError, error::classify_database_error};

/// PostgreSQL repository for atomic tenant provisioning and immutable audit.
#[derive(Clone, Debug)]
pub struct PostgresTenantProvisioning {
    pool: PgPool,
}

impl PostgresTenantProvisioning {
    /// Creates a provisioning repository using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl TenantProvisioningRepository for PostgresTenantProvisioning {
    type Error = PostgresTenantProvisioningError;

    /// Claims both immutable command identities before creating dependent state.
    ///
    /// `ON CONFLICT DO NOTHING` waits for a concurrent winner on either unique
    /// arbiter. A following statement therefore reads a fresh committed record
    /// and can distinguish an exact retry from a collision. The audit row uses
    /// a deferred tenant foreign key so the tenant and all three initial grants
    /// can be created later in the same transaction.
    async fn provision(
        &self,
        command: &TenantProvisioningCommand,
    ) -> Result<TenantProvisioningOutcome, Self::Error> {
        let mut transaction = self.pool.begin().await.map_err(repository_error)?;

        let insert = sqlx::query!(
            r#"
            INSERT INTO identity.tenant_provisionings
                (provisioning_id, tenant_id, provisioner_actor_id,
                 initial_administrator_actor_id, recorded_at_ms)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT DO NOTHING
            "#,
            command.provisioning_id().as_str(),
            command.tenant_id().as_str(),
            command.provisioner_actor_id().as_str(),
            command.initial_administrator_actor_id().as_str(),
            command.recorded_at().get(),
        )
        .execute(&mut *transaction)
        .await
        .map_err(repository_error)?;

        if insert.rows_affected() == 0 {
            let existing = sqlx::query!(
                r#"
                SELECT provisioning_id, tenant_id, provisioner_actor_id,
                       initial_administrator_actor_id, recorded_at_ms
                FROM identity.tenant_provisionings
                WHERE provisioning_id = $1 OR tenant_id = $2
                "#,
                command.provisioning_id().as_str(),
                command.tenant_id().as_str(),
            )
            .fetch_all(&mut *transaction)
            .await
            .map_err(repository_error)?;

            if existing.len() == 1 {
                let existing = &existing[0];
                if existing.provisioning_id == command.provisioning_id().as_str()
                    && existing.tenant_id == command.tenant_id().as_str()
                    && existing.provisioner_actor_id == command.provisioner_actor_id().as_str()
                    && existing.initial_administrator_actor_id
                        == command.initial_administrator_actor_id().as_str()
                    && existing.recorded_at_ms == command.recorded_at().get()
                {
                    transaction.commit().await.map_err(repository_error)?;
                    return Ok(TenantProvisioningOutcome::AlreadyProvisioned);
                }
            }

            return Err(PostgresTenantProvisioningError::IdentityCollision {
                provisioning_id: command.provisioning_id().clone(),
                tenant_id: command.tenant_id().clone(),
            });
        }

        sqlx::query!(
            "INSERT INTO metadata.tenants (tenant_id) VALUES ($1)",
            command.tenant_id().as_str(),
        )
        .execute(&mut *transaction)
        .await
        .map_err(repository_error)?;

        sqlx::query!(
            r#"
            INSERT INTO identity.tenant_action_grants (tenant_id, actor_id, action)
            VALUES
                ($1, $2, 'metadata.read'),
                ($1, $2, 'metadata.manage'),
                ($1, $2, 'access.manage')
            "#,
            command.tenant_id().as_str(),
            command.initial_administrator_actor_id().as_str(),
        )
        .execute(&mut *transaction)
        .await
        .map_err(repository_error)?;

        transaction.commit().await.map_err(repository_error)?;
        Ok(TenantProvisioningOutcome::Provisioned)
    }
}

fn repository_error(error: sqlx::Error) -> PostgresTenantProvisioningError {
    PostgresTenantProvisioningError::Repository(classify_database_error(error))
}

/// Failure to persist or resolve a tenant provisioning command.
#[derive(Debug)]
pub enum PostgresTenantProvisioningError {
    /// A provisioning or tenant identity already names different command content.
    IdentityCollision {
        /// Reused provisioning identity.
        provisioning_id: TenantProvisioningId,
        /// Reused or conflicting tenant identity.
        tenant_id: TenantId,
    },
    /// Lower-level PostgreSQL repository failure.
    Repository(PostgresRepositoryError),
}

impl fmt::Display for PostgresTenantProvisioningError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityCollision {
                provisioning_id,
                tenant_id,
            } => write!(
                formatter,
                "tenant provisioning identity collision for {provisioning_id}/{tenant_id}"
            ),
            Self::Repository(_) => formatter.write_str("PostgreSQL tenant provisioning failed"),
        }
    }
}

impl Error for PostgresTenantProvisioningError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::IdentityCollision { .. } => None,
            Self::Repository(error) => Some(error),
        }
    }
}

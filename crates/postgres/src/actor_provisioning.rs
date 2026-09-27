use std::{error::Error, fmt};

use fleetiq_application::{
    ActorProvisioningCommand, ActorProvisioningOutcome, ActorProvisioningRepository,
};
use fleetiq_domain::{ActorId, ActorProvisioningId};
use sqlx::PgPool;

use crate::{PostgresRepositoryError, error::classify_database_error};

/// PostgreSQL repository for atomic actor provisioning and immutable audit.
#[derive(Clone, Debug)]
pub struct PostgresActorProvisioning {
    pool: PgPool,
}

impl PostgresActorProvisioning {
    /// Creates an actor provisioning repository using the supplied pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl ActorProvisioningRepository for PostgresActorProvisioning {
    type Error = PostgresActorProvisioningError;

    /// Claims every immutable identity before creating the actor and mapping.
    ///
    /// `ON CONFLICT DO NOTHING` waits for a concurrent winner on the command,
    /// actor, or external-identity arbiter. A following statement reads a fresh
    /// committed snapshot and distinguishes an exact retry from a collision.
    /// Deferred foreign keys let immutable evidence be the transaction's first
    /// write while preserving all-or-nothing commit semantics.
    async fn provision(
        &self,
        command: &ActorProvisioningCommand,
    ) -> Result<ActorProvisioningOutcome, Self::Error> {
        let mut transaction = self.pool.begin().await.map_err(repository_error)?;
        let identity = command.external_identity();

        let insert = sqlx::query!(
            r#"
            INSERT INTO identity.actor_provisionings
                (provisioning_id, actor_id, authority_id, subject,
                 provisioner_actor_id, recorded_at_ms)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT DO NOTHING
            "#,
            command.provisioning_id().as_str(),
            command.actor_id().as_str(),
            identity.authority_id().as_str(),
            identity.subject().as_str(),
            command.provisioner_actor_id().as_str(),
            command.recorded_at().get(),
        )
        .execute(&mut *transaction)
        .await
        .map_err(repository_error)?;

        if insert.rows_affected() == 0 {
            let existing = sqlx::query!(
                r#"
                SELECT provisioning_id, actor_id, authority_id, subject,
                       provisioner_actor_id, recorded_at_ms
                FROM identity.actor_provisionings
                WHERE provisioning_id = $1
                   OR actor_id = $2
                   OR (authority_id = $3 AND subject = $4)
                "#,
                command.provisioning_id().as_str(),
                command.actor_id().as_str(),
                identity.authority_id().as_str(),
                identity.subject().as_str(),
            )
            .fetch_all(&mut *transaction)
            .await
            .map_err(repository_error)?;

            if existing.len() == 1 {
                let existing = &existing[0];
                if existing.provisioning_id == command.provisioning_id().as_str()
                    && existing.actor_id == command.actor_id().as_str()
                    && existing.authority_id == identity.authority_id().as_str()
                    && existing.subject == identity.subject().as_str()
                    && existing.provisioner_actor_id == command.provisioner_actor_id().as_str()
                    && existing.recorded_at_ms == command.recorded_at().get()
                {
                    transaction.commit().await.map_err(repository_error)?;
                    return Ok(ActorProvisioningOutcome::AlreadyProvisioned);
                }
            }

            return Err(PostgresActorProvisioningError::IdentityCollision {
                provisioning_id: command.provisioning_id().clone(),
                actor_id: command.actor_id().clone(),
            });
        }

        sqlx::query!(
            "INSERT INTO identity.actors (actor_id) VALUES ($1)",
            command.actor_id().as_str(),
        )
        .execute(&mut *transaction)
        .await
        .map_err(repository_error)?;

        sqlx::query!(
            r#"
            INSERT INTO identity.external_actor_identities
                (authority_id, subject, actor_id)
            VALUES ($1, $2, $3)
            "#,
            identity.authority_id().as_str(),
            identity.subject().as_str(),
            command.actor_id().as_str(),
        )
        .execute(&mut *transaction)
        .await
        .map_err(repository_error)?;

        transaction.commit().await.map_err(repository_error)?;
        Ok(ActorProvisioningOutcome::Provisioned)
    }
}

fn repository_error(error: sqlx::Error) -> PostgresActorProvisioningError {
    PostgresActorProvisioningError::Repository(classify_database_error(error))
}

/// Failure to persist or resolve an actor provisioning command.
#[derive(Debug)]
pub enum PostgresActorProvisioningError {
    /// A command, actor, or external identity already names different content.
    IdentityCollision {
        /// Reused provisioning identity.
        provisioning_id: ActorProvisioningId,
        /// Reused or conflicting actor identity.
        actor_id: ActorId,
    },
    /// Lower-level PostgreSQL repository failure.
    Repository(PostgresRepositoryError),
}

impl fmt::Display for PostgresActorProvisioningError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityCollision {
                provisioning_id,
                actor_id,
            } => write!(
                formatter,
                "actor provisioning identity collision for {provisioning_id}/{actor_id}"
            ),
            Self::Repository(_) => formatter.write_str("PostgreSQL actor provisioning failed"),
        }
    }
}

impl Error for PostgresActorProvisioningError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::IdentityCollision { .. } => None,
            Self::Repository(error) => Some(error),
        }
    }
}

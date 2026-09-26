use std::{error::Error, fmt};

use fleetiq_application::{
    PlatformBootstrapOutcome, PlatformBootstrapRepository, PlatformBootstrapRequest,
};
use sqlx::PgPool;

use crate::{PostgresRepositoryError, error::classify_database_error};

/// PostgreSQL repository for one-time platform bootstrap and immutable evidence.
#[derive(Clone, Debug)]
pub struct PostgresPlatformBootstrap {
    pool: PgPool,
}

impl PostgresPlatformBootstrap {
    /// Creates a platform bootstrap repository using the supplied pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl PlatformBootstrapRepository for PostgresPlatformBootstrap {
    type Error = PostgresPlatformBootstrapError;

    /// Claims the global bootstrap slot before creating dependent identity state.
    ///
    /// `ON CONFLICT DO NOTHING` waits for a concurrent winner. The following
    /// statement then sees that committed record under `READ COMMITTED` and can
    /// distinguish an exact retry from an attempt to replace the initial
    /// operator. Deferred foreign keys let immutable evidence be the first
    /// write while preserving all-or-nothing commit semantics.
    async fn bootstrap(
        &self,
        request: &PlatformBootstrapRequest,
    ) -> Result<PlatformBootstrapOutcome, Self::Error> {
        let mut transaction = self.pool.begin().await.map_err(repository_error)?;
        let identity = request.external_identity();

        let insert = sqlx::query!(
            r#"
            INSERT INTO identity.platform_bootstrap
                (singleton, actor_id, authority_id, subject, recorded_at_ms)
            VALUES (TRUE, $1, $2, $3, $4)
            ON CONFLICT DO NOTHING
            "#,
            request.actor_id().as_str(),
            identity.authority_id().as_str(),
            identity.subject().as_str(),
            request.recorded_at().get(),
        )
        .execute(&mut *transaction)
        .await
        .map_err(repository_error)?;

        if insert.rows_affected() == 0 {
            let existing = sqlx::query!(
                r#"
                SELECT actor_id, authority_id, subject
                FROM identity.platform_bootstrap
                WHERE singleton = TRUE
                "#,
            )
            .fetch_one(&mut *transaction)
            .await
            .map_err(repository_error)?;

            if existing.actor_id == request.actor_id().as_str()
                && existing.authority_id == identity.authority_id().as_str()
                && existing.subject == identity.subject().as_str()
            {
                transaction.commit().await.map_err(repository_error)?;
                return Ok(PlatformBootstrapOutcome::AlreadyBootstrapped);
            }

            return Err(PostgresPlatformBootstrapError::AlreadyInitialized);
        }

        sqlx::query!(
            "INSERT INTO identity.actors (actor_id) VALUES ($1)",
            request.actor_id().as_str(),
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
            request.actor_id().as_str(),
        )
        .execute(&mut *transaction)
        .await
        .map_err(repository_error)?;

        sqlx::query!(
            r#"
            INSERT INTO identity.platform_action_grants (actor_id, action)
            VALUES ($1, 'tenant.provision')
            "#,
            request.actor_id().as_str(),
        )
        .execute(&mut *transaction)
        .await
        .map_err(repository_error)?;

        transaction.commit().await.map_err(repository_error)?;
        Ok(PlatformBootstrapOutcome::Bootstrapped)
    }
}

fn repository_error(error: sqlx::Error) -> PostgresPlatformBootstrapError {
    PostgresPlatformBootstrapError::Repository(classify_database_error(error))
}

/// Failure to persist or resolve the one-time platform bootstrap operation.
#[derive(Debug)]
pub enum PostgresPlatformBootstrapError {
    /// Another immutable request has already initialized the platform.
    AlreadyInitialized,
    /// Lower-level PostgreSQL repository failure.
    Repository(PostgresRepositoryError),
}

impl fmt::Display for PostgresPlatformBootstrapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyInitialized => {
                formatter.write_str("the platform has already been initialized")
            }
            Self::Repository(_) => formatter.write_str("PostgreSQL platform bootstrap failed"),
        }
    }
}

impl Error for PostgresPlatformBootstrapError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::AlreadyInitialized => None,
            Self::Repository(error) => Some(error),
        }
    }
}

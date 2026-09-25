use fleetiq_application::ActorIdentityResolver;
use fleetiq_domain::{ActorId, ExternalIdentity};
use sqlx::PgPool;

use crate::{PostgresRepositoryError, error::corrupt};

/// PostgreSQL-backed exact external-identity resolver.
///
/// The adapter trusts only a validated [`ExternalIdentity`] established by an
/// authentication boundary. It performs no fallback across authorities,
/// subject normalization, actor creation, or tenant authorization.
#[derive(Clone, Debug)]
pub struct PostgresActorIdentityResolver {
    pool: PgPool,
}

impl PostgresActorIdentityResolver {
    /// Creates an identity resolver using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl ActorIdentityResolver for PostgresActorIdentityResolver {
    type Error = PostgresRepositoryError;

    async fn resolve(&self, identity: &ExternalIdentity) -> Result<Option<ActorId>, Self::Error> {
        let actor_id = sqlx::query_scalar!(
            r#"
            SELECT actor_id
            FROM identity.external_actor_identities
            WHERE authority_id = $1 AND subject = $2
            "#,
            identity.authority_id().as_str(),
            identity.subject().as_str(),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(PostgresRepositoryError::Database)?;

        actor_id
            .map(ActorId::new)
            .transpose()
            .map_err(|error| corrupt("identity.external_actor_identities.actor_id", error))
    }
}

/// Error returned by PostgreSQL external-identity resolution.
pub type PostgresActorIdentityResolverError = PostgresRepositoryError;

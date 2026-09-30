use std::{error::Error, fmt};

use fleetiq_access::{AccessDecision, PlatformAccessPolicy, PlatformAccessRequest, PlatformAction};
use sqlx::PgPool;

/// PostgreSQL-backed evaluator for exact platform action grants.
///
/// Each evaluation reads current database state for the complete actor and
/// action tuple. Actor existence and tenant-scoped grants do not imply platform
/// access, and absence returns [`AccessDecision::Deny`].
#[derive(Clone, Debug)]
pub struct PostgresPlatformAccessPolicy {
    pool: PgPool,
}

impl PostgresPlatformAccessPolicy {
    /// Creates a platform policy adapter using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl PlatformAccessPolicy for PostgresPlatformAccessPolicy {
    type Error = PostgresPlatformAccessPolicyError;

    /// Evaluates the exact actor and platform action against current grants.
    ///
    /// The deliberate query per operation makes revocation effective on the
    /// next decision. Caching requires a separately defined bounded-staleness
    /// and invalidation policy.
    async fn evaluate<'a>(
        &'a self,
        request: &'a PlatformAccessRequest,
    ) -> Result<AccessDecision, Self::Error> {
        let allowed = sqlx::query_scalar!(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM identity.platform_action_grants AS grant_state
                INNER JOIN identity.actors AS actor_state
                    ON actor_state.actor_id = grant_state.actor_id
                WHERE grant_state.actor_id = $1
                  AND grant_state.action = $2
                  AND actor_state.is_active
            ) AS "allowed!"
            "#,
            request.actor_id().as_str(),
            action_name(request.action()),
        )
        .fetch_one(&self.pool)
        .await
        .map_err(PostgresPlatformAccessPolicyError::new)?;

        Ok(if allowed {
            AccessDecision::Allow
        } else {
            AccessDecision::Deny
        })
    }
}

const fn action_name(action: PlatformAction) -> &'static str {
    match action {
        PlatformAction::ManagePlatformAccess => "platform.access.manage",
        PlatformAction::ManageIdentity => "identity.manage",
        PlatformAction::ProvisionTenant => "tenant.provision",
        PlatformAction::ManageTenantLifecycle => "tenant.lifecycle.manage",
    }
}

/// Failure to evaluate PostgreSQL-backed platform authorization policy.
#[derive(Debug)]
pub struct PostgresPlatformAccessPolicyError {
    source: sqlx::Error,
}

impl PostgresPlatformAccessPolicyError {
    fn new(source: sqlx::Error) -> Self {
        Self { source }
    }
}

impl fmt::Display for PostgresPlatformAccessPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PostgreSQL platform access policy evaluation failed")
    }
}

impl Error for PostgresPlatformAccessPolicyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

use std::{error::Error, fmt};

use crate::action_codec::tenant_action_name;
use fleetiq_access::{AccessDecision, TenantAccessPolicy, TenantAccessRequest};

use sqlx::PgPool;

/// PostgreSQL-backed evaluator for exact tenant action grants.
///
/// Each evaluation performs one complete-tuple existence query against current
/// database state. The adapter has no role hierarchy, wildcard, cached grant,
/// or default tenant behavior. Absence returns [`AccessDecision::Deny`].
#[derive(Clone, Debug)]
pub struct PostgresTenantAccessPolicy {
    pool: PgPool,
}

impl PostgresTenantAccessPolicy {
    /// Creates a policy adapter using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl TenantAccessPolicy for PostgresTenantAccessPolicy {
    type Error = PostgresTenantAccessPolicyError;

    /// Evaluates the exact actor, tenant, and action against current grants.
    ///
    /// This deliberate query-per-operation behavior prevents stale in-process
    /// grants after revocation. A cache may be added only with explicit bounded
    /// staleness and invalidation semantics.
    async fn evaluate<'a>(
        &'a self,
        request: &'a TenantAccessRequest,
    ) -> Result<AccessDecision, Self::Error> {
        let action = tenant_action_name(request.action());
        let allowed = sqlx::query_scalar!(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM identity.tenant_action_grants AS grant_state
                INNER JOIN identity.actors AS actor_state
                    ON actor_state.actor_id = grant_state.actor_id
                INNER JOIN metadata.tenants AS tenant_state
                    ON tenant_state.tenant_id = grant_state.tenant_id
                WHERE grant_state.tenant_id = $1
                  AND grant_state.actor_id = $2
                  AND grant_state.action = $3
                  AND actor_state.is_active
                  AND tenant_state.is_active
            ) AS "allowed!"
            "#,
            request.tenant_id().as_str(),
            request.actor_id().as_str(),
            action,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(PostgresTenantAccessPolicyError::new)?;

        Ok(if allowed {
            AccessDecision::Allow
        } else {
            AccessDecision::Deny
        })
    }
}

/// Failure to evaluate PostgreSQL-backed tenant authorization policy.
#[derive(Debug)]
pub struct PostgresTenantAccessPolicyError {
    source: sqlx::Error,
}

impl PostgresTenantAccessPolicyError {
    fn new(source: sqlx::Error) -> Self {
        Self { source }
    }
}

impl fmt::Display for PostgresTenantAccessPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PostgreSQL tenant access policy evaluation failed")
    }
}

impl Error for PostgresTenantAccessPolicyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

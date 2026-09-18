use std::{error::Error, fmt};

use fleetiq_application::{AccessDecision, TenantAccessPolicy, TenantAccessRequest, TenantAction};
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
        let action = action_name(request.action());
        let allowed = sqlx::query_scalar!(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM identity.tenant_action_grants
                WHERE tenant_id = $1
                  AND actor_id = $2
                  AND action = $3
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

const fn action_name(action: TenantAction) -> &'static str {
    match action {
        TenantAction::ReadMetadata => "metadata.read",
        TenantAction::ManageMetadata => "metadata.manage",
        TenantAction::ManageAccess => "access.manage",
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

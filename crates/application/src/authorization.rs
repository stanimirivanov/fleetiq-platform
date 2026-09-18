use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, TenantId};

/// Identifies the protected tenant operation for an authorization decision.
///
/// The initial actions form a small application-level vocabulary. Transport
/// scopes, identity-provider roles, and database grants must be translated to
/// these meanings at an adapter boundary instead of entering use cases
/// directly. The enum is deliberately closed so adding an action makes
/// exhaustive policy adapters review their decision.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TenantAction {
    /// Read tenant-owned asset and semantic metadata.
    ReadMetadata,
    /// Create or change tenant-owned asset and semantic metadata.
    ManageMetadata,
    /// Manage actors' access to a tenant.
    ManageAccess,
}

/// Describes one actor's attempt to perform an action in a selected tenant.
///
/// The actor must come from a successful authentication boundary. The tenant
/// may originate in request routing and is deliberately not treated as proof
/// that the actor belongs to or may access that tenant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantAccessRequest {
    actor_id: ActorId,
    tenant_id: TenantId,
    action: TenantAction,
}

impl TenantAccessRequest {
    /// Creates an authorization request from explicit actor, tenant, and action.
    pub const fn new(actor_id: ActorId, tenant_id: TenantId, action: TenantAction) -> Self {
        Self {
            actor_id,
            tenant_id,
            action,
        }
    }

    /// Returns the authenticated internal actor identity.
    pub const fn actor_id(&self) -> &ActorId {
        &self.actor_id
    }

    /// Returns the tenant selected for the protected operation.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the protected operation being requested.
    pub const fn action(&self) -> TenantAction {
        self.action
    }
}

/// Result returned by a tenant access policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessDecision {
    /// The exact actor, tenant, and action tuple is permitted now.
    Allow,
    /// No applicable grant permits the exact request.
    Deny,
}

/// Consumer-owned port for evaluating tenant authorization policy.
///
/// Implementations may use persisted memberships, an external policy engine,
/// or another trusted source. They must return [`AccessDecision::Deny`] when no
/// grant matches and must not infer access from authentication, tenant routing,
/// or resource identifiers alone.
pub trait TenantAccessPolicy: Send + Sync {
    /// Failure while loading or evaluating policy.
    type Error: Error + Send + Sync + 'static;

    /// Evaluates the complete request against current policy.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when policy cannot be evaluated. A
    /// missing grant is [`AccessDecision::Deny`], not an error.
    fn evaluate<'a>(
        &'a self,
        request: &'a TenantAccessRequest,
    ) -> impl Future<Output = Result<AccessDecision, Self::Error>> + Send + 'a;
}

/// Evidence that one exact tenant action passed the configured policy.
///
/// Protected application services should require this value instead of a bare
/// tenant identifier. It has no public constructor and must not be persisted,
/// serialized, cached, or reused for another action: authorization is checked
/// against current policy for each operation.
#[derive(Debug, Eq, PartialEq)]
#[must_use = "an authorization grant must be consumed by the protected operation"]
pub struct AuthorizedTenantAction {
    request: TenantAccessRequest,
}

impl AuthorizedTenantAction {
    /// Returns the authenticated actor authorized for the operation.
    pub const fn actor_id(&self) -> &ActorId {
        self.request.actor_id()
    }

    /// Returns the tenant in which the operation was authorized.
    pub const fn tenant_id(&self) -> &TenantId {
        self.request.tenant_id()
    }

    /// Returns the exact action authorized by the policy.
    pub const fn action(&self) -> TenantAction {
        self.request.action()
    }
}

/// Authorizes tenant operations through one application-wide policy boundary.
#[derive(Clone, Debug)]
pub struct TenantAuthorizationService<P> {
    policy: P,
}

impl<P> TenantAuthorizationService<P> {
    /// Creates a service backed by the supplied policy implementation.
    pub const fn new(policy: P) -> Self {
        Self { policy }
    }
}

impl<P> TenantAuthorizationService<P>
where
    P: TenantAccessPolicy,
{
    /// Authorizes one exact actor, tenant, and action tuple.
    ///
    /// Call this for every protected operation. Both explicit policy denial and
    /// policy evaluation failure prevent creation of authorization evidence.
    ///
    /// # Errors
    ///
    /// Returns [`TenantAuthorizationError::Denied`] if policy does not grant
    /// the request, or [`TenantAuthorizationError::Policy`] if policy cannot be
    /// evaluated.
    pub async fn authorize(
        &self,
        request: TenantAccessRequest,
    ) -> Result<AuthorizedTenantAction, TenantAuthorizationError<P::Error>> {
        match self.policy.evaluate(&request).await {
            Ok(AccessDecision::Allow) => Ok(AuthorizedTenantAction { request }),
            Ok(AccessDecision::Deny) => Err(TenantAuthorizationError::Denied),
            Err(error) => Err(TenantAuthorizationError::Policy(error)),
        }
    }
}

/// Failure to authorize a protected tenant operation.
#[derive(Debug)]
pub enum TenantAuthorizationError<E> {
    /// Current policy does not grant the requested action.
    Denied,
    /// Current policy could not be evaluated and access failed closed.
    Policy(E),
}

impl<E> fmt::Display for TenantAuthorizationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Denied => formatter.write_str("tenant access denied"),
            Self::Policy(_) => formatter.write_str("tenant access policy could not be evaluated"),
        }
    }
}

impl<E> Error for TenantAuthorizationError<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Denied => None,
            Self::Policy(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, future::ready};

    use super::*;

    struct ExactGrantPolicy {
        granted: TenantAccessRequest,
    }

    impl TenantAccessPolicy for ExactGrantPolicy {
        type Error = Infallible;

        fn evaluate<'a>(
            &'a self,
            request: &'a TenantAccessRequest,
        ) -> impl Future<Output = Result<AccessDecision, Self::Error>> + Send + 'a {
            ready(Ok(if request == &self.granted {
                AccessDecision::Allow
            } else {
                AccessDecision::Deny
            }))
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct PolicyUnavailable;

    impl fmt::Display for PolicyUnavailable {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("policy unavailable")
        }
    }

    impl Error for PolicyUnavailable {}

    struct FailingPolicy;

    impl TenantAccessPolicy for FailingPolicy {
        type Error = PolicyUnavailable;

        fn evaluate<'a>(
            &'a self,
            _request: &'a TenantAccessRequest,
        ) -> impl Future<Output = Result<AccessDecision, Self::Error>> + Send + 'a {
            ready(Err(PolicyUnavailable))
        }
    }

    #[tokio::test]
    async fn exact_policy_grant_produces_matching_authorization_evidence() {
        let granted = request("actor-a", "tenant-a", TenantAction::ManageMetadata);
        let service = TenantAuthorizationService::new(ExactGrantPolicy {
            granted: granted.clone(),
        });

        let authorization = service
            .authorize(granted)
            .await
            .expect("exact request should be authorized");

        assert_eq!(authorization.actor_id().as_str(), "actor-a");
        assert_eq!(authorization.tenant_id().as_str(), "tenant-a");
        assert_eq!(authorization.action(), TenantAction::ManageMetadata);
    }

    #[tokio::test]
    async fn grant_for_one_tenant_does_not_authorize_another_tenant() {
        let service = TenantAuthorizationService::new(ExactGrantPolicy {
            granted: request("actor-a", "tenant-a", TenantAction::ReadMetadata),
        });

        let error = service
            .authorize(request("actor-a", "tenant-b", TenantAction::ReadMetadata))
            .await
            .expect_err("tenant selection must not imply authorization");

        assert!(matches!(error, TenantAuthorizationError::Denied));
    }

    #[tokio::test]
    async fn grant_for_one_actor_does_not_authorize_another_actor() {
        let service = TenantAuthorizationService::new(ExactGrantPolicy {
            granted: request("actor-a", "tenant-a", TenantAction::ReadMetadata),
        });

        let error = service
            .authorize(request("actor-b", "tenant-a", TenantAction::ReadMetadata))
            .await
            .expect_err("one actor's grant must not authorize another actor");

        assert!(matches!(error, TenantAuthorizationError::Denied));
    }

    #[tokio::test]
    async fn grant_for_one_action_does_not_authorize_another_action() {
        let service = TenantAuthorizationService::new(ExactGrantPolicy {
            granted: request("actor-a", "tenant-a", TenantAction::ReadMetadata),
        });

        let error = service
            .authorize(request("actor-a", "tenant-a", TenantAction::ManageMetadata))
            .await
            .expect_err("one action grant must not imply another");

        assert!(matches!(error, TenantAuthorizationError::Denied));
    }

    #[tokio::test]
    async fn policy_failure_fails_closed_without_becoming_a_denial() {
        let service = TenantAuthorizationService::new(FailingPolicy);

        let error = service
            .authorize(request("actor-a", "tenant-a", TenantAction::ReadMetadata))
            .await
            .expect_err("policy failure must not authorize access");

        assert!(matches!(
            error,
            TenantAuthorizationError::Policy(PolicyUnavailable)
        ));
        assert!(error.source().is_some());
    }

    fn request(actor: &str, tenant: &str, action: TenantAction) -> TenantAccessRequest {
        TenantAccessRequest::new(
            ActorId::new(actor).expect("actor ID should be valid"),
            TenantId::new(tenant).expect("tenant ID should be valid"),
            action,
        )
    }
}

use std::{error::Error, fmt, future::Future, marker::PhantomData};

use fleetiq_domain::{ActorId, TenantId};

use crate::AuthenticatedActor;

/// Identifies the protected tenant operation for an authorization decision.
///
/// The initial actions form a small access-policy vocabulary. Transport
/// scopes, identity-provider roles, and database grants must be translated to
/// these meanings at an adapter boundary instead of entering use cases
/// directly. The enum is deliberately closed so adding an action makes
/// exhaustive policy adapters review their decision.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TenantAction {
    /// Read tenant-owned asset and semantic metadata.
    ReadMetadata,
    /// Read tenant-owned telemetry history and interpretation provenance.
    ReadTelemetry,
    /// Create or change tenant-owned asset and semantic metadata.
    ManageMetadata,
    /// Manage actors' access to a tenant.
    ManageAccess,
    /// Register and admit tenant-owned executable protocol packages.
    ManageProtocols,
}

mod sealed {
    pub trait Sealed {}
}

/// A tenant action whose authorization evidence carries its exact action in its type.
///
/// This trait is sealed: callers cannot define an action that bypasses the
/// closed [`TenantAction`] vocabulary or its policy evaluation.
pub trait TenantActionMarker: sealed::Sealed {
    /// Persisted action evaluated by the tenant policy.
    const ACTION: TenantAction;
}

/// Permission to read tenant-owned metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadMetadata;

impl sealed::Sealed for ReadMetadata {}
impl TenantActionMarker for ReadMetadata {
    const ACTION: TenantAction = TenantAction::ReadMetadata;
}

/// Permission to read telemetry, independent of metadata or protocol management.
///
/// ```compile_fail
/// use fleetiq_access::{AuthorizedTenant, ReadMetadata, ReadTelemetry};
/// fn read_telemetry(_: AuthorizedTenant<ReadTelemetry>) {}
/// fn metadata_is_not_telemetry(grant: AuthorizedTenant<ReadMetadata>) {
///     read_telemetry(grant);
/// }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadTelemetry;
impl sealed::Sealed for ReadTelemetry {}
impl TenantActionMarker for ReadTelemetry {
    const ACTION: TenantAction = TenantAction::ReadTelemetry;
}

/// Permission to manage tenant-owned metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManageMetadata;

impl sealed::Sealed for ManageMetadata {}
impl TenantActionMarker for ManageMetadata {
    const ACTION: TenantAction = TenantAction::ManageMetadata;
}

/// Permission to administer tenant access grants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManageAccess;

impl sealed::Sealed for ManageAccess {}
impl TenantActionMarker for ManageAccess {
    const ACTION: TenantAction = TenantAction::ManageAccess;
}

/// Permission to register and admit protocol code, independent of metadata writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManageProtocols;
impl sealed::Sealed for ManageProtocols {}
impl TenantActionMarker for ManageProtocols {
    const ACTION: TenantAction = TenantAction::ManageProtocols;
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
    const fn new(actor_id: ActorId, tenant_id: TenantId, action: TenantAction) -> Self {
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
/// Protected services require the marker for their action, making a wrong
/// action impossible to route to a use case. Evidence has no public constructor
/// and must not be persisted, serialized, cached, or reused: authorization is
/// checked against current policy for each operation.
///
/// ```compile_fail
/// use fleetiq_access::{AuthorizedTenant, ManageMetadata, ReadMetadata};
/// fn read_metadata(_: AuthorizedTenant<ReadMetadata>) {}
/// fn wrong_action(grant: AuthorizedTenant<ManageMetadata>) {
///     read_metadata(grant);
/// }
/// ```
///
/// ```compile_fail
/// use fleetiq_access::{AuthorizedTenant, ReadMetadata};
/// let _grant = AuthorizedTenant::<ReadMetadata> {
///     request: todo!(),
///     marker: std::marker::PhantomData,
/// };
/// ```
#[derive(Debug, Eq, PartialEq)]
#[must_use = "an authorization grant must be consumed by the protected operation"]
pub struct AuthorizedTenant<A: TenantActionMarker> {
    request: TenantAccessRequest,
    marker: PhantomData<A>,
}

impl<A: TenantActionMarker> AuthorizedTenant<A> {
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
        A::ACTION
    }
}

/// Authorizes tenant operations through one access-policy boundary.
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
    /// Authorizes one authenticated actor for an exact tenant and typed action.
    ///
    /// Call this for every protected operation. Both explicit policy denial and
    /// policy evaluation failure prevent creation of authorization evidence.
    ///
    /// # Errors
    ///
    /// Returns [`TenantAuthorizationError::Denied`] if policy does not grant
    /// the request, or [`TenantAuthorizationError::Policy`] if policy cannot be
    /// evaluated.
    pub async fn authorize<A: TenantActionMarker>(
        &self,
        actor: AuthenticatedActor,
        tenant_id: TenantId,
    ) -> Result<AuthorizedTenant<A>, TenantAuthorizationError<P::Error>> {
        let request = TenantAccessRequest::new(actor.into_actor_id(), tenant_id, A::ACTION);
        match self.policy.evaluate(&request).await {
            Ok(AccessDecision::Allow) => Ok(AuthorizedTenant {
                request,
                marker: PhantomData,
            }),
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
            .authorize::<ManageMetadata>(
                authenticated_actor("actor-a"),
                TenantId::new("tenant-a").expect("tenant ID should be valid"),
            )
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
            .authorize::<ReadMetadata>(
                authenticated_actor("actor-a"),
                TenantId::new("tenant-b").expect("tenant ID should be valid"),
            )
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
            .authorize::<ReadMetadata>(
                authenticated_actor("actor-b"),
                TenantId::new("tenant-a").expect("tenant ID should be valid"),
            )
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
            .authorize::<ManageMetadata>(
                authenticated_actor("actor-a"),
                TenantId::new("tenant-a").expect("tenant ID should be valid"),
            )
            .await
            .expect_err("one action grant must not imply another");

        assert!(matches!(error, TenantAuthorizationError::Denied));
    }

    #[tokio::test]
    async fn policy_failure_fails_closed_without_becoming_a_denial() {
        let service = TenantAuthorizationService::new(FailingPolicy);

        let error = service
            .authorize::<ReadMetadata>(
                authenticated_actor("actor-a"),
                TenantId::new("tenant-a").expect("tenant ID should be valid"),
            )
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

    fn authenticated_actor(actor: &str) -> AuthenticatedActor {
        AuthenticatedActor::from_trusted_actor_id(
            ActorId::new(actor).expect("actor ID should be valid"),
        )
    }
}

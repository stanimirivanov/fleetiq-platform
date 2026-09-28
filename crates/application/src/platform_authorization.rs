use std::{error::Error, fmt, future::Future};

use fleetiq_domain::ActorId;

use crate::{AccessDecision, AuthenticatedActor};

/// Identifies a platform-wide administrative operation.
///
/// Platform actions are deliberately separate from [`crate::TenantAction`]
/// because they do not operate inside an existing tenant boundary. Adding an
/// action requires exhaustive review by every policy adapter.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PlatformAction {
    /// Grant or revoke exact platform actions with immutable audit evidence.
    ManagePlatformAccess,
    /// Create internal actors and bind their initial external identity.
    ManageIdentity,
    /// Create a tenant and establish its initial administration state.
    ProvisionTenant,
    /// Activate or suspend an existing tenant with immutable audit evidence.
    ManageTenantLifecycle,
}

/// Describes an authenticated actor's platform authorization request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformAccessRequest {
    actor_id: ActorId,
    action: PlatformAction,
}

impl PlatformAccessRequest {
    const fn new(actor_id: ActorId, action: PlatformAction) -> Self {
        Self { actor_id, action }
    }

    /// Returns the authenticated internal actor identity.
    pub const fn actor_id(&self) -> &ActorId {
        &self.actor_id
    }

    /// Returns the requested platform operation.
    pub const fn action(&self) -> PlatformAction {
        self.action
    }
}

/// Consumer-owned port for evaluating platform authorization policy.
///
/// Implementations must evaluate the complete actor and action tuple. Actor
/// existence, authentication, tenant membership, and tenant-scoped grants do
/// not imply platform authority.
pub trait PlatformAccessPolicy: Send + Sync {
    /// Failure while loading or evaluating policy.
    type Error: Error + Send + Sync + 'static;

    /// Evaluates the complete request against current platform policy.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when policy cannot be evaluated. A
    /// missing grant is [`AccessDecision::Deny`], not an error.
    fn evaluate<'a>(
        &'a self,
        request: &'a PlatformAccessRequest,
    ) -> impl Future<Output = Result<AccessDecision, Self::Error>> + Send + 'a;
}

/// Evidence that one exact platform action passed the configured policy.
///
/// The value has no public constructor and must not be persisted, serialized,
/// cached, or reused for another operation. Protected platform services consume
/// it as single-use authorization evidence.
#[derive(Debug, Eq, PartialEq)]
#[must_use = "a platform authorization grant must be consumed by the protected operation"]
pub struct AuthorizedPlatformAction {
    request: PlatformAccessRequest,
}

impl AuthorizedPlatformAction {
    /// Returns the authenticated actor authorized for the operation.
    pub const fn actor_id(&self) -> &ActorId {
        self.request.actor_id()
    }

    /// Returns the exact platform action authorized by policy.
    pub const fn action(&self) -> PlatformAction {
        self.request.action()
    }
}

/// Authorizes platform operations through one policy boundary.
#[derive(Clone, Debug)]
pub struct PlatformAuthorizationService<P> {
    policy: P,
}

impl<P> PlatformAuthorizationService<P> {
    /// Creates a service backed by the supplied policy implementation.
    pub const fn new(policy: P) -> Self {
        Self { policy }
    }
}

impl<P> PlatformAuthorizationService<P>
where
    P: PlatformAccessPolicy,
{
    /// Authorizes one authenticated actor for one exact platform action.
    ///
    /// Authentication and tenant-scoped authorization cannot create this
    /// evidence. Both explicit denial and policy failure fail closed.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformAuthorizationError::Denied`] when the exact grant is
    /// absent, or [`PlatformAuthorizationError::Policy`] when policy cannot be
    /// evaluated.
    pub async fn authorize(
        &self,
        actor: AuthenticatedActor,
        action: PlatformAction,
    ) -> Result<AuthorizedPlatformAction, PlatformAuthorizationError<P::Error>> {
        let request = PlatformAccessRequest::new(actor.into_actor_id(), action);
        match self.policy.evaluate(&request).await {
            Ok(AccessDecision::Allow) => Ok(AuthorizedPlatformAction { request }),
            Ok(AccessDecision::Deny) => Err(PlatformAuthorizationError::Denied),
            Err(error) => Err(PlatformAuthorizationError::Policy(error)),
        }
    }
}

/// Failure to authorize a platform operation.
#[derive(Debug)]
pub enum PlatformAuthorizationError<E> {
    /// Current policy does not grant the requested platform action.
    Denied,
    /// Current policy could not be evaluated and access failed closed.
    Policy(E),
}

impl<E> fmt::Display for PlatformAuthorizationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Denied => formatter.write_str("platform access denied"),
            Self::Policy(_) => formatter.write_str("platform access policy could not be evaluated"),
        }
    }
}

impl<E> Error for PlatformAuthorizationError<E>
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

    struct ExactPolicy {
        actor_id: ActorId,
        action: PlatformAction,
    }

    impl PlatformAccessPolicy for ExactPolicy {
        type Error = Infallible;

        fn evaluate<'a>(
            &'a self,
            request: &'a PlatformAccessRequest,
        ) -> impl Future<Output = Result<AccessDecision, Self::Error>> + Send + 'a {
            ready(Ok(
                if request.actor_id() == &self.actor_id && request.action() == self.action {
                    AccessDecision::Allow
                } else {
                    AccessDecision::Deny
                },
            ))
        }
    }

    struct FailingPolicy;

    impl PlatformAccessPolicy for FailingPolicy {
        type Error = PolicyFailure;

        fn evaluate<'a>(
            &'a self,
            _request: &'a PlatformAccessRequest,
        ) -> impl Future<Output = Result<AccessDecision, Self::Error>> + Send + 'a {
            ready(Err(PolicyFailure))
        }
    }

    #[derive(Debug)]
    struct PolicyFailure;

    impl fmt::Display for PolicyFailure {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("policy failure")
        }
    }

    impl Error for PolicyFailure {}

    #[tokio::test]
    async fn exact_grant_produces_matching_platform_evidence() {
        let service = PlatformAuthorizationService::new(ExactPolicy {
            actor_id: actor_id("actor-a"),
            action: PlatformAction::ProvisionTenant,
        });

        let authorization = service
            .authorize(
                authenticated_actor("actor-a"),
                PlatformAction::ProvisionTenant,
            )
            .await
            .expect("exact platform grant should authorize");

        assert_eq!(authorization.actor_id().as_str(), "actor-a");
        assert_eq!(authorization.action(), PlatformAction::ProvisionTenant);
    }

    #[tokio::test]
    async fn grant_for_one_actor_does_not_authorize_another_actor() {
        let service = PlatformAuthorizationService::new(ExactPolicy {
            actor_id: actor_id("actor-a"),
            action: PlatformAction::ProvisionTenant,
        });

        let error = service
            .authorize(
                authenticated_actor("actor-b"),
                PlatformAction::ProvisionTenant,
            )
            .await
            .expect_err("another actor must be denied");

        assert!(matches!(error, PlatformAuthorizationError::Denied));
    }

    #[tokio::test]
    async fn policy_failure_fails_closed() {
        let error = PlatformAuthorizationService::new(FailingPolicy)
            .authorize(
                authenticated_actor("actor-a"),
                PlatformAction::ProvisionTenant,
            )
            .await
            .expect_err("policy failure must not authorize platform access");

        assert!(matches!(error, PlatformAuthorizationError::Policy(_)));
    }

    fn actor_id(value: &str) -> ActorId {
        ActorId::new(value).expect("valid actor ID")
    }

    fn authenticated_actor(value: &str) -> AuthenticatedActor {
        AuthenticatedActor::from_trusted_actor_id(actor_id(value))
    }
}

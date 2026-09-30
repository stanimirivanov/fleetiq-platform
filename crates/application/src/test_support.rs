//! Test-only path through authentication for protected application use cases.

use std::{
    convert::Infallible,
    future::{Future, ready},
};

use fleetiq_domain::{ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId};

use crate::{
    ActorIdentityResolver, AuthenticatedActor, CallerAuthenticationService,
    ExternalIdentityAuthenticator,
};

struct FixedAuthenticator;

impl ExternalIdentityAuthenticator for FixedAuthenticator {
    type Credential = ();
    type Error = Infallible;

    fn authenticate(
        &self,
        &(): &(),
    ) -> impl Future<Output = Result<Option<ExternalIdentity>, Infallible>> + Send {
        ready(Ok(Some(test_identity())))
    }
}

struct FixedResolver(ActorId);

impl ActorIdentityResolver for FixedResolver {
    type Error = Infallible;

    fn resolve<'a>(
        &'a self,
        identity: &'a ExternalIdentity,
    ) -> impl Future<Output = Result<Option<ActorId>, Infallible>> + Send + 'a {
        ready(Ok((identity == &test_identity()).then(|| self.0.clone())))
    }
}

fn test_identity() -> ExternalIdentity {
    ExternalIdentity::new(
        IdentityAuthorityId::new("application-test").expect("valid authority ID"),
        ExternalIdentitySubject::new("registered-actor").expect("valid subject"),
    )
}

/// Creates actor evidence through the public authentication boundary.
pub(crate) async fn authenticated_actor(actor_id: ActorId) -> AuthenticatedActor {
    CallerAuthenticationService::new(FixedAuthenticator, FixedResolver(actor_id))
        .authenticate(&())
        .await
        .expect("test identity should authenticate")
}

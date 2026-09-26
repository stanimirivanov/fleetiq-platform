use std::{
    convert::Infallible,
    future::{Future, ready},
};

use fleetiq_application::{
    ActorIdentityResolver, AuthenticatedActor, CallerAuthenticationService,
    ExternalIdentityAuthenticator,
};
use fleetiq_domain::{ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId};

struct TestAuthenticator;

impl ExternalIdentityAuthenticator for TestAuthenticator {
    type Credential = ActorId;
    type Error = Infallible;

    fn authenticate<'a>(
        &'a self,
        credential: &'a Self::Credential,
    ) -> impl Future<Output = Result<Option<ExternalIdentity>, Self::Error>> + Send + 'a {
        ready(Ok(Some(ExternalIdentity::new(
            IdentityAuthorityId::new("integration-test").expect("valid test authority"),
            ExternalIdentitySubject::new(credential.as_str()).expect("valid test subject"),
        ))))
    }
}

struct TestResolver;

impl ActorIdentityResolver for TestResolver {
    type Error = Infallible;

    fn resolve<'a>(
        &'a self,
        identity: &'a ExternalIdentity,
    ) -> impl Future<Output = Result<Option<ActorId>, Self::Error>> + Send + 'a {
        ready(Ok(Some(
            ActorId::new(identity.subject().as_str()).expect("valid test actor ID"),
        )))
    }
}

pub async fn authenticated_actor(actor_id: &str) -> AuthenticatedActor {
    CallerAuthenticationService::new(TestAuthenticator, TestResolver)
        .authenticate(&ActorId::new(actor_id).expect("valid test actor ID"))
        .await
        .expect("test authentication should succeed")
}

use std::{
    error::Error,
    fmt,
    future::{Future, ready},
    sync::{Arc, Mutex},
};

use fleetiq_domain::{ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId};

use crate::ActorIdentityResolver;

use super::{
    CallerAuthenticationError, CallerAuthenticationService, ExternalIdentityAuthenticator,
};

struct TestCredential(String);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TestAuthenticatorError;

impl fmt::Display for TestAuthenticatorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("test authenticator failed")
    }
}

impl Error for TestAuthenticatorError {}

#[derive(Clone, Debug)]
enum AuthenticatorResult {
    Authenticated(ExternalIdentity),
    Rejected,
    Failed,
}

#[derive(Clone, Debug)]
struct TestAuthenticator {
    result: AuthenticatorResult,
}

impl ExternalIdentityAuthenticator for TestAuthenticator {
    type Credential = TestCredential;
    type Error = TestAuthenticatorError;

    fn authenticate<'a>(
        &'a self,
        credential: &'a Self::Credential,
    ) -> impl Future<Output = Result<Option<ExternalIdentity>, Self::Error>> + Send + 'a {
        assert_eq!(credential.0, "opaque-secret");
        ready(match &self.result {
            AuthenticatorResult::Authenticated(identity) => Ok(Some(identity.clone())),
            AuthenticatorResult::Rejected => Ok(None),
            AuthenticatorResult::Failed => Err(TestAuthenticatorError),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TestResolverError;

impl fmt::Display for TestResolverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("test resolver failed")
    }
}

impl Error for TestResolverError {}

#[derive(Clone, Debug, Default)]
struct RecordingResolver {
    calls: Arc<Mutex<Vec<ExternalIdentity>>>,
    actor_id: Option<ActorId>,
    fail: bool,
}

impl RecordingResolver {
    fn resolving(actor_id: &str) -> Self {
        Self {
            actor_id: Some(ActorId::new(actor_id).expect("valid actor ID")),
            ..Self::default()
        }
    }

    fn failing() -> Self {
        Self {
            fail: true,
            ..Self::default()
        }
    }

    fn calls(&self) -> Vec<ExternalIdentity> {
        self.calls.lock().expect("resolver lock poisoned").clone()
    }
}

impl ActorIdentityResolver for RecordingResolver {
    type Error = TestResolverError;

    fn resolve<'a>(
        &'a self,
        identity: &'a ExternalIdentity,
    ) -> impl Future<Output = Result<Option<ActorId>, Self::Error>> + Send + 'a {
        self.calls
            .lock()
            .expect("resolver lock poisoned")
            .push(identity.clone());
        ready(if self.fail {
            Err(TestResolverError)
        } else {
            Ok(self.actor_id.clone())
        })
    }
}

#[tokio::test]
async fn validates_credentials_before_resolving_the_internal_actor() {
    let identity = external_identity();
    let resolver = RecordingResolver::resolving("actor-a");
    let service = CallerAuthenticationService::new(
        TestAuthenticator {
            result: AuthenticatorResult::Authenticated(identity.clone()),
        },
        resolver.clone(),
    );

    let authenticated = service
        .authenticate(&credential())
        .await
        .expect("registered authenticated principal should succeed");

    assert_eq!(authenticated.actor_id().as_str(), "actor-a");
    assert_eq!(resolver.calls(), vec![identity]);
}

#[tokio::test]
async fn rejected_credentials_never_reach_identity_resolution() {
    let resolver = RecordingResolver::resolving("actor-a");
    let service = CallerAuthenticationService::new(
        TestAuthenticator {
            result: AuthenticatorResult::Rejected,
        },
        resolver.clone(),
    );

    let error = service
        .authenticate(&credential())
        .await
        .expect_err("rejected credentials must fail authentication");

    assert!(matches!(error, CallerAuthenticationError::Rejected));
    assert_eq!(resolver.calls().len(), 0);
}

#[tokio::test]
async fn authenticated_but_unregistered_principal_fails_closed() {
    let resolver = RecordingResolver::default();
    let service = CallerAuthenticationService::new(
        TestAuthenticator {
            result: AuthenticatorResult::Authenticated(external_identity()),
        },
        resolver.clone(),
    );

    let error = service
        .authenticate(&credential())
        .await
        .expect_err("unregistered principal must not receive actor evidence");

    assert!(matches!(error, CallerAuthenticationError::Unregistered));
    assert_eq!(resolver.calls(), vec![external_identity()]);
}

#[tokio::test]
async fn preserves_authenticator_and_resolver_failures_as_error_sources() {
    let authenticator_error = CallerAuthenticationService::new(
        TestAuthenticator {
            result: AuthenticatorResult::Failed,
        },
        RecordingResolver::default(),
    )
    .authenticate(&credential())
    .await
    .expect_err("authenticator failure should be returned");
    assert!(matches!(
        authenticator_error,
        CallerAuthenticationError::Authenticator(TestAuthenticatorError)
    ));
    assert!(authenticator_error.source().is_some());

    let resolver_error = CallerAuthenticationService::new(
        TestAuthenticator {
            result: AuthenticatorResult::Authenticated(external_identity()),
        },
        RecordingResolver::failing(),
    )
    .authenticate(&credential())
    .await
    .expect_err("resolver failure should be returned");
    assert!(matches!(
        resolver_error,
        CallerAuthenticationError::Resolver(TestResolverError)
    ));
    assert!(resolver_error.source().is_some());
}

fn credential() -> TestCredential {
    TestCredential("opaque-secret".to_owned())
}

fn external_identity() -> ExternalIdentity {
    ExternalIdentity::new(
        IdentityAuthorityId::new("workforce").expect("valid authority ID"),
        ExternalIdentitySubject::new("subject-a").expect("valid subject"),
    )
}

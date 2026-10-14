use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Request, header},
};
use fleetiq_access::{
    ActorIdentityResolver, CallerAuthenticationService, DiscoveredTenant, TenantDiscoveryPage,
};
use fleetiq_domain::{ActorId, ExternalIdentity};
use std::{
    convert::Infallible,
    future::{Future, ready},
    io,
    sync::atomic::{AtomicUsize, Ordering},
};
use tower::ServiceExt;

const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PATH: &str = "/api/v1/me/tenants";

struct Resolver;
impl ActorIdentityResolver for Resolver {
    type Error = Infallible;
    fn resolve<'a>(
        &'a self,
        _: &'a ExternalIdentity,
    ) -> impl Future<Output = Result<Option<ActorId>, Self::Error>> + Send + 'a {
        ready(Ok(Some(ActorId::new("actor-a").unwrap())))
    }
}
struct Repository {
    calls: Arc<AtomicUsize>,
    fail: bool,
}
impl TenantDiscoveryRepository for Repository {
    type Error = io::Error;
    fn find_page<'a>(
        &'a self,
        actor: &'a ActorId,
        _: &'a TenantDiscoveryQuery,
    ) -> impl Future<Output = Result<TenantDiscoveryPage, Self::Error>> + Send + 'a {
        assert_eq!(actor.as_str(), "actor-a");
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            return ready(Err(io::Error::other("private database detail")));
        }
        ready(Ok(TenantDiscoveryPage::new(
            vec![DiscoveredTenant::new(
                TenantId::new("tenant-a").unwrap(),
                vec![TenantAction::ReadMetadata],
            )],
            None,
        )))
    }
}
fn router(calls: Arc<AtomicUsize>, fail: bool) -> Router {
    let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "reader")
        .unwrap();
    let authentication = CallerAuthenticationService::new(
        crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
            config.bearer_identity().unwrap().clone(),
        ),
        Resolver,
    );
    discovery_router(authentication, Repository { calls, fail })
}
fn request(path: &str, authenticated: bool) -> Request<Body> {
    let mut request = Request::get(path);
    if authenticated {
        request = request.header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
    }
    request.body(Body::empty()).unwrap()
}
#[tokio::test]
async fn discovery_matches_published_example_and_is_not_cached() {
    let calls = Arc::new(AtomicUsize::new(0));
    let response = router(calls.clone(), false)
        .oneshot(request(PATH, true))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let body = to_bytes(response.into_body(), 8192).await.unwrap();
    crate::contract_test_support::assert_published_example(&body, PATH);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn authentication_precedes_validation_and_bad_queries_do_not_read_policy() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = router(calls.clone(), false);
    for suffix in [
        "?limit=0",
        "?limit=101",
        "?limit=65536",
        "?limit=no",
        "?after=",
        "?actor_id=actor-b",
        "?limit=1&limit=2",
    ] {
        let path = format!("{PATH}{suffix}");
        let anonymous = app.clone().oneshot(request(&path, false)).await.unwrap();
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(anonymous.headers()[header::CACHE_CONTROL], "no-store");
        let invalid = app.clone().oneshot(request(&path, true)).await.unwrap();
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(invalid.headers()[header::CACHE_CONTROL], "no-store");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn errors_are_redacted_and_unsupported_methods_do_not_read_policy() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = router(calls.clone(), true);
    let response = app.clone().oneshot(request(PATH, true)).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let body = to_bytes(response.into_body(), 8192).await.unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("private"));
    let response = app
        .oneshot(Request::post(PATH).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

use std::{
    convert::Infallible,
    error::Error,
    future::ready,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use fleetiq_access::{AccessDecision, ActorIdentityResolver, TenantAccessRequest, TenantAction};
use fleetiq_domain::{
    ActorId, AssetName, AssetTypeId, AssetTypeRef, DefinitionVersion, ExternalIdentifier,
    ExternalIdentifierAuthority, ExternalIdentifierKind, ExternalIdentifierValue, ExternalIdentity,
};
use tower::ServiceExt;

use super::*;

const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PATH: &str = "/api/v1/tenants/tenant-a/assets/asset-1";

#[derive(Clone, Default)]
struct Calls {
    resolver: Arc<AtomicUsize>,
    policy: Arc<AtomicUsize>,
    asset: Arc<AtomicUsize>,
}

struct TestResolver {
    actor: Option<ActorId>,
    calls: Calls,
}

impl ActorIdentityResolver for TestResolver {
    type Error = Infallible;

    fn resolve<'a>(
        &'a self,
        identity: &'a ExternalIdentity,
    ) -> impl Future<Output = Result<Option<ActorId>, Self::Error>> + Send + 'a {
        assert_eq!(identity.authority_id().as_str(), "workload");
        assert_eq!(identity.subject().as_str(), "reader");
        self.calls.resolver.fetch_add(1, Ordering::SeqCst);
        ready(Ok(self.actor.clone()))
    }
}

#[derive(Clone, Copy, Debug)]
struct TestFailure;

impl std::fmt::Display for TestFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("private adapter detail")
    }
}

impl Error for TestFailure {}

struct TestPolicy {
    decision: Result<AccessDecision, TestFailure>,
    calls: Calls,
}

impl TenantAccessPolicy for TestPolicy {
    type Error = TestFailure;

    fn evaluate<'a>(
        &'a self,
        request: &'a TenantAccessRequest,
    ) -> impl Future<Output = Result<AccessDecision, Self::Error>> + Send + 'a {
        assert_eq!(request.actor_id().as_str(), "actor-a");
        assert_eq!(request.tenant_id().as_str(), "tenant-a");
        assert_eq!(request.action(), TenantAction::ReadMetadata);
        self.calls.policy.fetch_add(1, Ordering::SeqCst);
        ready(self.decision)
    }
}

struct TestAssets {
    asset: Option<Asset>,
    calls: Calls,
}

impl AssetRepository for TestAssets {
    type Error = Infallible;

    fn insert<'a>(
        &'a self,
        _: &'a Asset,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        ready(Ok(()))
    }

    fn find_by_id<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        asset_id: &'a AssetId,
    ) -> impl Future<Output = Result<Option<Asset>, Self::Error>> + Send + 'a {
        assert_eq!(tenant_id.as_str(), "tenant-a");
        assert_eq!(asset_id.as_str(), "asset-1");
        self.calls.asset.fetch_add(1, Ordering::SeqCst);
        ready(Ok(self.asset.clone()))
    }
}

fn sample_asset() -> Asset {
    let mut asset = Asset::new(
        TenantId::new("tenant-a").unwrap(),
        AssetId::new("asset-1").unwrap(),
        AssetTypeRef::new(
            AssetTypeId::new("generic.machine").unwrap(),
            DefinitionVersion::new(1).unwrap(),
        ),
        AssetName::new("Primary machine").unwrap(),
    );
    asset
        .add_external_identifier(ExternalIdentifier::new(
            ExternalIdentifierKind::new("manufacturer.serial-number").unwrap(),
            Some(ExternalIdentifierAuthority::new("maker-a").unwrap()),
            ExternalIdentifierValue::new("SN-100").unwrap(),
        ))
        .unwrap();
    asset
}

fn router(
    actor: Option<ActorId>,
    decision: Result<AccessDecision, TestFailure>,
    asset: Option<Asset>,
    calls: Calls,
) -> Router {
    let bearer = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "reader")
        .unwrap()
        .bearer_identity()
        .unwrap()
        .clone();
    asset_router(
        CallerAuthenticationService::new(
            ConfiguredBearerAuthenticator::new(bearer),
            TestResolver {
                actor,
                calls: calls.clone(),
            },
        ),
        TenantAuthorizationService::new(TestPolicy {
            decision,
            calls: calls.clone(),
        }),
        AssetApplicationService::new(TestAssets { asset, calls }),
    )
}

fn request(path: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::get(path);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::empty()).unwrap()
}

fn actor() -> ActorId {
    ActorId::new("actor-a").unwrap()
}

#[tokio::test]
async fn missing_or_invalid_credentials_never_reach_identity_resolution() {
    let calls = Calls::default();
    let router = router(
        Some(actor()),
        Ok(AccessDecision::Allow),
        Some(sample_asset()),
        calls.clone(),
    );
    for candidate in [None, Some("b".repeat(64))] {
        let response = router
            .clone()
            .oneshot(request(PATH, candidate.as_deref()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers()[header::WWW_AUTHENTICATE],
            "Bearer realm=\"FleetIQ\""
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    assert_eq!(calls.resolver.load(Ordering::SeqCst), 0);
    assert_eq!(calls.policy.load(Ordering::SeqCst), 0);
    assert_eq!(calls.asset.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unregistered_identity_and_missing_grant_fail_closed() {
    let calls = Calls::default();
    let no_actor = router(
        None,
        Ok(AccessDecision::Allow),
        Some(sample_asset()),
        calls.clone(),
    );
    let response = no_actor.oneshot(request(PATH, Some(TOKEN))).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(calls.policy.load(Ordering::SeqCst), 0);

    let denied = router(
        Some(actor()),
        Ok(AccessDecision::Deny),
        Some(sample_asset()),
        calls.clone(),
    );
    let response = denied.oneshot(request(PATH, Some(TOKEN))).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(calls.asset.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn policy_failure_never_reads_an_asset_or_exposes_adapter_detail() {
    let calls = Calls::default();
    let router = router(
        Some(actor()),
        Err(TestFailure),
        Some(sample_asset()),
        calls.clone(),
    );
    let response = router.oneshot(request(PATH, Some(TOKEN))).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("private adapter detail"));
    assert_eq!(calls.asset.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn authorized_asset_query_returns_a_transport_document() {
    let calls = Calls::default();
    let router = router(
        Some(actor()),
        Ok(AccessDecision::Allow),
        Some(sample_asset()),
        calls.clone(),
    );
    let response = router.oneshot(request(PATH, Some(TOKEN))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        to_bytes(response.into_body(), 2048).await.unwrap(),
        r#"{"id":"asset-1","tenant_id":"tenant-a","name":"Primary machine","asset_type":{"id":"generic.machine","version":1},"external_identifiers":[{"kind":"manufacturer.serial-number","authority":"maker-a","value":"SN-100"}]}"#
    );
    assert_eq!(calls.resolver.load(Ordering::SeqCst), 1);
    assert_eq!(calls.policy.load(Ordering::SeqCst), 1);
    assert_eq!(calls.asset.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn authorized_missing_asset_is_not_found_and_invalid_id_is_rejected() {
    let calls = Calls::default();
    let router = router(
        Some(actor()),
        Ok(AccessDecision::Allow),
        None,
        calls.clone(),
    );
    let response = router
        .clone()
        .oneshot(request(PATH, Some(TOKEN)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = router
        .oneshot(request("/api/v1/tenants/%20/assets/asset-1", Some(TOKEN)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(calls.asset.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn protected_path_rejects_other_methods_with_problem_details() {
    let router = router(
        Some(actor()),
        Ok(AccessDecision::Allow),
        Some(sample_asset()),
        Calls::default(),
    );
    let response = router
        .oneshot(Request::post(PATH).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/problem+json"
    );
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn configured_route_reads_only_an_asset_with_current_postgres_grant(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a'), ('tenant-b')")
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ('actor-a')")
        .execute(&pool)
        .await?;
    sqlx::query(
        "INSERT INTO identity.external_actor_identities \
         (authority_id, subject, actor_id) VALUES ('workload', 'reader', 'actor-a')",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "INSERT INTO identity.tenant_action_grants \
         (tenant_id, actor_id, action) VALUES ('tenant-a', 'actor-a', 'metadata.read')",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "INSERT INTO metadata.asset_type_definitions \
         (tenant_id, type_id, version, display_name, property_count) \
         VALUES ('tenant-a', 'generic.machine', 1, 'Machine', 0)",
    )
    .execute(&pool)
    .await?;
    PostgresAssetRepository::new(pool.clone())
        .insert(&sample_asset())
        .await?;

    let bearer = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")?
        .with_bearer_identity(TOKEN, "workload", "reader")?
        .bearer_identity()
        .expect("configured credential")
        .clone();
    let router = postgres_asset_router(pool.clone(), bearer);
    assert_eq!(
        router
            .clone()
            .oneshot(request(PATH, Some(TOKEN)))
            .await?
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/api/v1/tenants/tenant-b/assets/asset-1",
                Some(TOKEN),
            ))
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );

    sqlx::query(
        "DELETE FROM identity.tenant_action_grants \
         WHERE tenant_id = 'tenant-a' AND actor_id = 'actor-a' AND action = 'metadata.read'",
    )
    .execute(&pool)
    .await?;
    assert_eq!(
        router.oneshot(request(PATH, Some(TOKEN))).await?.status(),
        StatusCode::FORBIDDEN
    );
    Ok(())
}

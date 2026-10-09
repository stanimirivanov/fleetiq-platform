use std::{
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
    ActorId, DeviceEndpoint, DeviceEndpointId, DeviceEndpointKindId, DeviceEndpointName,
    ExternalIdentity,
};
use tower::ServiceExt;

use super::*;

const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PATH: &str = "/api/v1/tenants/tenant-a/assets/gateway-1/device";

#[derive(Clone, Copy, Debug)]
struct TestFailure;

impl std::fmt::Display for TestFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("private repository detail")
    }
}

impl Error for TestFailure {}

struct TestResolver {
    registered: bool,
}

impl ActorIdentityResolver for TestResolver {
    type Error = TestFailure;

    fn resolve<'a>(
        &'a self,
        identity: &'a ExternalIdentity,
    ) -> impl Future<Output = Result<Option<ActorId>, Self::Error>> + Send + 'a {
        assert_eq!(identity.authority_id().as_str(), "workload");
        assert_eq!(identity.subject().as_str(), "reader");
        ready(Ok(self
            .registered
            .then(|| ActorId::new("actor-a").expect("valid test actor"))))
    }
}

struct TestPolicy {
    allowed: bool,
}

impl TenantAccessPolicy for TestPolicy {
    type Error = TestFailure;

    fn evaluate<'a>(
        &'a self,
        request: &'a TenantAccessRequest,
    ) -> impl Future<Output = Result<AccessDecision, Self::Error>> + Send + 'a {
        assert_eq!(request.actor_id().as_str(), "actor-a");
        assert_eq!(request.action(), TenantAction::ReadMetadata);
        ready(Ok(
            if self.allowed && request.tenant_id().as_str() == "tenant-a" {
                AccessDecision::Allow
            } else {
                AccessDecision::Deny
            },
        ))
    }
}

struct TestDevices {
    result: Result<Option<CommunicationDevice>, TestFailure>,
    calls: Arc<AtomicUsize>,
}

impl DeviceRepository for TestDevices {
    type Error = TestFailure;

    fn insert<'a>(
        &'a self,
        _: &'a CommunicationDevice,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        ready(Ok(()))
    }

    fn find_by_asset<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        asset_id: &'a AssetId,
    ) -> impl Future<Output = Result<Option<CommunicationDevice>, Self::Error>> + Send + 'a {
        assert_eq!(tenant_id.as_str(), "tenant-a");
        assert_eq!(asset_id.as_str(), "gateway-1");
        self.calls.fetch_add(1, Ordering::SeqCst);
        ready(self.result.clone())
    }
}

fn sample_device() -> CommunicationDevice {
    let mut device = CommunicationDevice::new(
        TenantId::new("tenant-a").unwrap(),
        AssetId::new("gateway-1").unwrap(),
    );
    for (id, kind, name) in [
        ("onboard", "device.onboard", "Onboard measurements"),
        ("can-primary", "bus.can", "Primary CAN interface"),
    ] {
        device
            .add_endpoint(DeviceEndpoint::new(
                DeviceEndpointId::new(id).unwrap(),
                DeviceEndpointKindId::new(kind).unwrap(),
                DeviceEndpointName::new(name).unwrap(),
            ))
            .unwrap();
    }
    device
}

fn router(
    registered: bool,
    allowed: bool,
    result: Result<Option<CommunicationDevice>, TestFailure>,
    calls: Arc<AtomicUsize>,
) -> Router {
    let bearer = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "reader")
        .unwrap()
        .bearer_identity()
        .unwrap()
        .clone();
    device_router(
        CallerAuthenticationService::new(
            ConfiguredBearerAuthenticator::new(bearer),
            TestResolver { registered },
        ),
        TenantAuthorizationService::new(TestPolicy { allowed }),
        DeviceApplicationService::new(TestDevices { result, calls }),
    )
}

fn request(path: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::get(path);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn authorized_device_returns_endpoints_in_declaration_order() {
    let calls = Arc::new(AtomicUsize::new(0));
    let router = router(true, true, Ok(Some(sample_device())), calls.clone());
    let response = router.oneshot(request(PATH, Some(TOKEN))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        to_bytes(response.into_body(), 2048).await.unwrap(),
        r#"{"tenant_id":"tenant-a","asset_id":"gateway-1","endpoints":[{"id":"onboard","kind":"device.onboard","name":"Onboard measurements"},{"id":"can-primary","kind":"bus.can","name":"Primary CAN interface"}]}"#
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn credentials_and_current_policy_guard_device_lookup() {
    let calls = Arc::new(AtomicUsize::new(0));
    for (registered, allowed, token, path, expected) in [
        (true, true, None, PATH, StatusCode::UNAUTHORIZED),
        (
            true,
            true,
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            PATH,
            StatusCode::UNAUTHORIZED,
        ),
        (false, true, Some(TOKEN), PATH, StatusCode::UNAUTHORIZED),
        (true, false, Some(TOKEN), PATH, StatusCode::FORBIDDEN),
        (
            true,
            true,
            Some(TOKEN),
            "/api/v1/tenants/tenant-b/assets/gateway-1/device",
            StatusCode::FORBIDDEN,
        ),
    ] {
        let response = router(
            registered,
            allowed,
            Ok(Some(sample_device())),
            calls.clone(),
        )
        .oneshot(request(path, token))
        .await
        .unwrap();
        assert_eq!(response.status(), expected);
        if expected == StatusCode::UNAUTHORIZED {
            assert_eq!(
                response.headers()[header::WWW_AUTHENTICATE],
                "Bearer realm=\"FleetIQ\""
            );
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn invalid_id_missing_capability_and_repository_failure_are_bounded() {
    let calls = Arc::new(AtomicUsize::new(0));
    let missing = router(true, true, Ok(None), calls.clone());
    assert_eq!(
        missing
            .clone()
            .oneshot(request(PATH, Some(TOKEN)))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        missing
            .oneshot(request(
                "/api/v1/tenants/%20/assets/gateway-1/device",
                Some(TOKEN)
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let failed = router(true, true, Err(TestFailure), calls);
    let response = failed.oneshot(request(PATH, Some(TOKEN))).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("private repository detail"));
}

#[tokio::test]
async fn device_path_rejects_other_methods() {
    let router = router(true, true, Ok(Some(sample_device())), Arc::default());
    let response = router
        .oneshot(Request::post(PATH).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn configured_device_query_uses_current_postgres_grant(
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
    for tenant in ["tenant-a", "tenant-b"] {
        sqlx::query(
            "INSERT INTO metadata.asset_type_definitions \
             (tenant_id, type_id, version, display_name, property_count) \
             VALUES ($1, 'communication.gateway', 1, 'Communication gateway', 0)",
        )
        .bind(tenant)
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO metadata.assets \
             (tenant_id, asset_id, type_id, type_version, display_name) \
             VALUES ($1, 'gateway-1', 'communication.gateway', 1, 'Gateway')",
        )
        .bind(tenant)
        .execute(&pool)
        .await?;
    }
    PostgresDeviceRepository::new(pool.clone())
        .insert(&sample_device())
        .await?;

    let bearer = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")?
        .with_bearer_identity(TOKEN, "workload", "reader")?
        .bearer_identity()
        .expect("configured credential")
        .clone();
    let router = postgres_device_router(
        pool.clone(),
        fleetiq_access::CallerAuthenticationService::new(
            crate::bearer_authentication::ConfiguredBearerAuthenticator::new(bearer),
            fleetiq_postgres::PostgresActorIdentityResolver::new(pool.clone()),
        ),
    );
    let response = router.clone().oneshot(request(PATH, Some(TOKEN))).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 2048).await?,
        r#"{"tenant_id":"tenant-a","asset_id":"gateway-1","endpoints":[{"id":"onboard","kind":"device.onboard","name":"Onboard measurements"},{"id":"can-primary","kind":"bus.can","name":"Primary CAN interface"}]}"#
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/api/v1/tenants/tenant-b/assets/gateway-1/device",
                Some(TOKEN)
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

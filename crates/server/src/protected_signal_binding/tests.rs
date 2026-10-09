use std::{
    error::Error,
    future::{Future, ready},
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
    ActorId, AssetId, DefinitionVersion, DeviceEndpointId, EffectiveInterval, ExternalIdentity,
    PropertyDefinitionId, PropertyDefinitionRef, RecordRevision, RecordedRevision, SignalSource,
    SignalTarget, SourceSignalId,
};
use fleetiq_metadata::{
    SignalBindingSnapshotPage, SignalBindingSnapshotPageQuery, SignalBindingSnapshotQuery,
};
use tower::ServiceExt;

use crate::contract_test_support::assert_published_example;

use super::*;

const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PATH: &str = "/api/v1/tenants/tenant-a/signal-bindings/binding-1/revisions/1";
const SNAPSHOT_PATH: &str = "/api/v1/tenants/tenant-a/signal-bindings?device_asset_id=gateway-1&endpoint_id=can-primary&signal_id=bus.voltage&effective_at_ms=1500&known_at_ms=2500&limit=1";
const TARGET_PATH: &str = "/api/v1/tenants/tenant-a/assets/power-system-1/signal-bindings?effective_at_ms=1500&known_at_ms=2500&limit=1";
const EXPECTED_JSON: &str = r#"{"id":"binding-1","tenant_id":"tenant-a","revision":"1","recorded_at_ms":2000,"source":{"device_asset_id":"gateway-1","endpoint_id":"can-primary","signal_id":"bus.voltage"},"target":{"asset_id":"power-system-1","property":{"id":"electrical.voltage","version":1}},"effective_interval":{"start_ms":1000,"end_ms":3000}}"#;

#[derive(Clone, Copy, Debug)]
struct TestFailure;

impl std::fmt::Display for TestFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("private binding adapter detail")
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
            .then(|| ActorId::new("actor-a").unwrap())))
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

struct TestBindings {
    result: Result<Option<SignalBinding>, TestFailure>,
    calls: Arc<AtomicUsize>,
}

impl SignalBindingRepository for TestBindings {
    type Error = TestFailure;

    fn insert_revision(
        &self,
        _: &SignalBinding,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        ready(Ok(()))
    }

    fn find_revision(
        &self,
        tenant_id: &TenantId,
        binding_id: &SignalBindingId,
        revision: RecordRevision,
    ) -> impl Future<Output = Result<Option<SignalBinding>, Self::Error>> + Send {
        assert_eq!(tenant_id.as_str(), "tenant-a");
        assert_eq!(binding_id.as_str(), "binding-1");
        assert_eq!(revision.get(), 1);
        self.calls.fetch_add(1, Ordering::SeqCst);
        ready(self.result.clone())
    }

    fn find_for_source(
        &self,
        _: &TenantId,
        _: &SignalSource,
        _: SignalBindingSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<SignalBinding>, Self::Error>> + Send {
        ready(Ok(Vec::new()))
    }

    fn find_for_source_page(
        &self,
        tenant_id: &TenantId,
        source: &SignalSource,
        query: &SignalBindingSnapshotPageQuery,
    ) -> impl Future<Output = Result<SignalBindingSnapshotPage, Self::Error>> + Send {
        assert_eq!(tenant_id.as_str(), "tenant-a");
        assert_eq!(source.device_asset_id().as_str(), "gateway-1");
        assert_eq!(source.endpoint_id().as_str(), "can-primary");
        assert_eq!(source.signal_id().as_str(), "bus.voltage");
        assert_eq!(query.snapshot().effective_at().get(), 1_500);
        assert_eq!(query.snapshot().known_at().get(), 2_500);
        self.calls.fetch_add(1, Ordering::SeqCst);
        ready(
            self.result
                .clone()
                .map(|binding| SignalBindingSnapshotPage::new(binding.into_iter().collect(), None)),
        )
    }

    fn find_for_target_page(
        &self,
        tenant_id: &TenantId,
        asset_id: &AssetId,
        query: &SignalBindingSnapshotPageQuery,
    ) -> impl Future<Output = Result<Option<SignalBindingSnapshotPage>, Self::Error>> + Send {
        assert_eq!(tenant_id.as_str(), "tenant-a");
        assert_eq!(query.snapshot().effective_at().get(), 1_500);
        assert_eq!(query.snapshot().known_at().get(), 2_500);
        self.calls.fetch_add(1, Ordering::SeqCst);
        ready(self.result.clone().map(|binding| {
            (asset_id.as_str() != "missing-asset")
                .then(|| SignalBindingSnapshotPage::new(binding.into_iter().collect(), None))
        }))
    }
}

fn sample_binding(revision: u64, target_asset: &str) -> SignalBinding {
    SignalBinding::new(
        TenantId::new("tenant-a").unwrap(),
        SignalBindingId::new("binding-1").unwrap(),
        RecordedRevision::new(
            RecordRevision::new(revision).unwrap(),
            UnixTimeMillis::new(if revision == 1 { 2_000 } else { 4_000 }),
        ),
        SignalSource::new(
            AssetId::new("gateway-1").unwrap(),
            DeviceEndpointId::new("can-primary").unwrap(),
            SourceSignalId::new("bus.voltage").unwrap(),
        ),
        SignalTarget::new(
            AssetId::new(target_asset).unwrap(),
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("electrical.voltage").unwrap(),
                DefinitionVersion::new(1).unwrap(),
            ),
        ),
        EffectiveInterval::new(
            UnixTimeMillis::new(1_000),
            (revision == 1).then_some(UnixTimeMillis::new(3_000)),
        )
        .unwrap(),
    )
}

fn router(
    registered: bool,
    allowed: bool,
    result: Result<Option<SignalBinding>, TestFailure>,
    calls: Arc<AtomicUsize>,
) -> Router {
    let bearer = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "reader")
        .unwrap()
        .bearer_identity()
        .unwrap()
        .clone();
    signal_binding_router(
        CallerAuthenticationService::new(
            ConfiguredBearerAuthenticator::new(bearer),
            TestResolver { registered },
        ),
        TenantAuthorizationService::new(TestPolicy { allowed }),
        SignalBindingApplicationService::new(TestBindings { result, calls }),
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
async fn authorized_query_preserves_source_target_and_both_time_dimensions() {
    let calls = Arc::new(AtomicUsize::new(0));
    let response = router(
        true,
        true,
        Ok(Some(sample_binding(1, "power-system-1"))),
        calls.clone(),
    )
    .oneshot(request(PATH, Some(TOKEN)))
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        to_bytes(response.into_body(), 2048).await.unwrap(),
        EXPECTED_JSON
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn credentials_and_current_tenant_grant_guard_binding_lookup() {
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
            "/api/v1/tenants/tenant-b/signal-bindings/binding-1/revisions/1",
            StatusCode::FORBIDDEN,
        ),
    ] {
        let response = router(
            registered,
            allowed,
            Ok(Some(sample_binding(1, "power-system-1"))),
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
async fn invalid_path_and_missing_exact_revision_are_bounded() {
    let calls = Arc::new(AtomicUsize::new(0));
    let router = router(true, true, Ok(None), calls.clone());
    for path in [
        "/api/v1/tenants/tenant-a/signal-bindings/binding-1/revisions/0",
        "/api/v1/tenants/tenant-a/signal-bindings/binding-1/revisions/+1",
        "/api/v1/tenants/tenant-a/signal-bindings/binding-1/revisions/nope",
        "/api/v1/tenants/tenant-a/signal-bindings/binding-1/revisions/18446744073709551616",
        "/api/v1/tenants/tenant-a/signal-bindings/%20bad/revisions/1",
    ] {
        let response = router
            .clone()
            .oneshot(request(path, Some(TOKEN)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        router
            .oneshot(request(PATH, Some(TOKEN)))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn repository_error_is_redacted_and_other_methods_are_rejected() {
    let response = router(true, true, Err(TestFailure), Arc::default())
        .oneshot(request(PATH, Some(TOKEN)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        !String::from_utf8_lossy(&to_bytes(response.into_body(), 1024).await.unwrap())
            .contains("private binding adapter detail")
    );

    let response = router(true, true, Ok(None), Arc::default())
        .oneshot(Request::post(PATH).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn source_snapshot_requires_authorization_and_valid_bounded_parameters() {
    let calls = Arc::new(AtomicUsize::new(0));
    let test_router = router(
        true,
        true,
        Ok(Some(sample_binding(1, "power-system-1"))),
        calls.clone(),
    );
    let response = test_router
        .clone()
        .oneshot(request(SNAPSHOT_PATH, Some(TOKEN)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("\"signal_id\":\"bus.voltage\""));
    assert!(body.contains("\"revision\":\"1\""));
    assert!(body.contains("\"next_after\":null"));
    for path in [
        "/api/v1/tenants/tenant-a/signal-bindings",
        "/api/v1/tenants/tenant-a/signal-bindings?device_asset_id=gateway-1&endpoint_id=can-primary&signal_id=bus.voltage&effective_at_ms=1&known_at_ms=2&limit=0",
        "/api/v1/tenants/tenant-a/signal-bindings?device_asset_id=gateway-1&endpoint_id=can-primary&signal_id=bus.voltage&effective_at_ms=1&known_at_ms=2&limit=101",
        "/api/v1/tenants/tenant-a/signal-bindings?device_asset_id=%20bad&endpoint_id=can-primary&signal_id=bus.voltage&effective_at_ms=1&known_at_ms=2",
    ] {
        assert_eq!(
            test_router
                .clone()
                .oneshot(request(path, Some(TOKEN)))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        test_router
            .clone()
            .oneshot(request(SNAPSHOT_PATH, None))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        router(true, false, Ok(None), Arc::default())
            .oneshot(request(SNAPSHOT_PATH, Some(TOKEN)))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let empty = router(true, true, Ok(None), Arc::default())
        .oneshot(request(SNAPSHOT_PATH, Some(TOKEN)))
        .await
        .unwrap();
    assert_eq!(empty.status(), StatusCode::OK);
    assert!(
        String::from_utf8_lossy(&to_bytes(empty.into_body(), 1024).await.unwrap())
            .contains("\"bindings\":[]")
    );
}

#[tokio::test]
async fn target_snapshot_distinguishes_missing_and_unbound_assets_and_validates_input() {
    let calls = Arc::new(AtomicUsize::new(0));
    let test_router = router(
        true,
        true,
        Ok(Some(sample_binding(1, "power-system-1"))),
        calls.clone(),
    );
    let response = test_router
        .clone()
        .oneshot(request(TARGET_PATH, Some(TOKEN)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("\"asset_id\":\"power-system-1\""));
    assert!(body.contains("\"revision\":\"1\""));
    assert!(body.contains("\"next_after\":null"));
    for path in [
        "/api/v1/tenants/tenant-a/assets/power-system-1/signal-bindings",
        "/api/v1/tenants/tenant-a/assets/power-system-1/signal-bindings?effective_at_ms=1500&known_at_ms=2500&limit=0",
        "/api/v1/tenants/tenant-a/assets/power-system-1/signal-bindings?effective_at_ms=1500&known_at_ms=2500&limit=101",
        "/api/v1/tenants/tenant-a/assets/power-system-1/signal-bindings?effective_at_ms=1500&known_at_ms=2500&after=%20",
        "/api/v1/tenants/tenant-a/assets/%20/signal-bindings?effective_at_ms=1500&known_at_ms=2500",
    ] {
        assert_eq!(
            test_router
                .clone()
                .oneshot(request(path, Some(TOKEN)))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST,
            "{path}"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        test_router
            .clone()
            .oneshot(request(TARGET_PATH, None))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        router(true, false, Ok(None), Arc::default())
            .oneshot(request(TARGET_PATH, Some(TOKEN)))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let missing = router(true, true, Ok(None), Arc::default())
        .oneshot(request(
            "/api/v1/tenants/tenant-a/assets/missing-asset/signal-bindings?effective_at_ms=1500&known_at_ms=2500",
            Some(TOKEN),
        ))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let empty = router(true, true, Ok(None), Arc::default())
        .oneshot(request(TARGET_PATH, Some(TOKEN)))
        .await
        .unwrap();
    assert_eq!(empty.status(), StatusCode::OK);
    assert!(
        String::from_utf8_lossy(&to_bytes(empty.into_body(), 1024).await.unwrap())
            .contains("\"bindings\":[]")
    );
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn configured_query_reads_exact_revision_and_checks_current_postgres_grant(
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
    seed_topology(&pool).await?;

    let bindings = PostgresSignalBindingRepository::new(pool.clone());
    bindings
        .insert_revision(&sample_binding(1, "power-system-1"))
        .await?;
    bindings
        .insert_revision(&sample_binding(2, "power-system-2"))
        .await?;

    let bearer = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")?
        .with_bearer_identity(TOKEN, "workload", "reader")?
        .bearer_identity()
        .expect("configured credential")
        .clone();
    let router = postgres_signal_binding_router(
        pool.clone(),
        fleetiq_access::CallerAuthenticationService::new(
            crate::bearer_authentication::ConfiguredBearerAuthenticator::new(bearer),
            fleetiq_postgres::PostgresActorIdentityResolver::new(pool.clone()),
        ),
    );
    let target_early = router
        .clone()
        .oneshot(request(TARGET_PATH, Some(TOKEN)))
        .await?;
    assert_eq!(target_early.status(), StatusCode::OK);
    let body = to_bytes(target_early.into_body(), 4096).await?;
    assert!(String::from_utf8_lossy(&body).contains("\"revision\":\"1\""));
    let target_late = router.clone().oneshot(request(
        "/api/v1/tenants/tenant-a/assets/power-system-1/signal-bindings?effective_at_ms=1500&known_at_ms=5000",
        Some(TOKEN),
    )).await?;
    assert_eq!(target_late.status(), StatusCode::OK);
    assert!(
        String::from_utf8_lossy(&to_bytes(target_late.into_body(), 4096).await?)
            .contains("\"bindings\":[]")
    );
    let moved_target = router.clone().oneshot(request(
        "/api/v1/tenants/tenant-a/assets/power-system-2/signal-bindings?effective_at_ms=1500&known_at_ms=5000",
        Some(TOKEN),
    )).await?;
    assert_eq!(moved_target.status(), StatusCode::OK);
    let body = to_bytes(moved_target.into_body(), 4096).await?;
    assert!(String::from_utf8_lossy(&body).contains("\"revision\":\"2\""));
    assert_eq!(
        router.clone().oneshot(request(
            "/api/v1/tenants/tenant-a/assets/missing-asset/signal-bindings?effective_at_ms=1500&known_at_ms=5000",
            Some(TOKEN),
        )).await?.status(),
        StatusCode::NOT_FOUND
    );
    let snapshot = router.clone().oneshot(request(
        "/api/v1/tenants/tenant-a/signal-bindings?device_asset_id=gateway-1&endpoint_id=can-primary&signal_id=bus.voltage&effective_at_ms=1500&known_at_ms=5000",
        Some(TOKEN),
    )).await?;
    assert_eq!(snapshot.status(), StatusCode::OK);
    let snapshot_body = to_bytes(snapshot.into_body(), 4096).await?;
    assert!(String::from_utf8_lossy(&snapshot_body).contains("\"revision\":\"2\""));
    assert!(String::from_utf8_lossy(&snapshot_body).contains("\"asset_id\":\"power-system-2\""));
    let response = router.clone().oneshot(request(PATH, Some(TOKEN))).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 2048).await?, EXPECTED_JSON);

    let response = router
        .clone()
        .oneshot(request(
            "/api/v1/tenants/tenant-a/signal-bindings/binding-1/revisions/2",
            Some(TOKEN),
        ))
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 2048).await?;
    assert!(String::from_utf8_lossy(&body).contains("\"asset_id\":\"power-system-2\""));
    assert!(String::from_utf8_lossy(&body).contains("\"revision\":\"2\""));
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/api/v1/tenants/tenant-a/signal-bindings/binding-1/revisions/3",
                Some(TOKEN),
            ))
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/api/v1/tenants/tenant-b/signal-bindings/binding-1/revisions/1",
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

/// The fixture is protocol-neutral; only topology and exact property support
/// needed by the binding repository are inserted here.
async fn seed_topology(pool: &PgPool) -> Result<(), Box<dyn Error>> {
    // The deferred property-count constraint sees the complete definition at commit.
    let mut definitions = pool.begin().await?;
    sqlx::query(
        "INSERT INTO metadata.property_definitions \
         (tenant_id, property_id, version, display_name, value_kind, canonical_unit) \
         VALUES ('tenant-a', 'electrical.voltage', 1, 'Voltage', 'decimal', 'volt')",
    )
    .execute(&mut *definitions)
    .await?;
    sqlx::query(
        "INSERT INTO metadata.asset_type_definitions \
         (tenant_id, type_id, version, display_name, property_count) VALUES \
         ('tenant-a', 'communication.gateway', 1, 'Gateway', 0), \
         ('tenant-a', 'generic.power-system', 1, 'Power system', 1)",
    )
    .execute(&mut *definitions)
    .await?;
    sqlx::query(
        "INSERT INTO metadata.asset_type_properties \
         (tenant_id, type_id, type_version, property_id, property_version, property_ordinal) \
         VALUES ('tenant-a', 'generic.power-system', 1, 'electrical.voltage', 1, 1)",
    )
    .execute(&mut *definitions)
    .await?;
    definitions.commit().await?;
    sqlx::query(
        "INSERT INTO metadata.assets \
         (tenant_id, asset_id, type_id, type_version, display_name) VALUES \
         ('tenant-a', 'gateway-1', 'communication.gateway', 1, 'Gateway'), \
         ('tenant-a', 'power-system-1', 'generic.power-system', 1, 'Power system one'), \
         ('tenant-a', 'power-system-2', 'generic.power-system', 1, 'Power system two')",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO metadata.communication_devices (tenant_id, device_asset_id) \
         VALUES ('tenant-a', 'gateway-1')",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO metadata.device_endpoints \
         (tenant_id, device_asset_id, endpoint_id, endpoint_kind_id, \
          display_name, endpoint_ordinal) \
         VALUES ('tenant-a', 'gateway-1', 'can-primary', 'bus.can', 'Primary CAN', 1)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[tokio::test]
async fn source_and_target_snapshots_match_published_examples() {
    for (request_path, contract_path) in [
        (SNAPSHOT_PATH, "/api/v1/tenants/{tenant_id}/signal-bindings"),
        (
            TARGET_PATH,
            "/api/v1/tenants/{tenant_id}/assets/{asset_id}/signal-bindings",
        ),
    ] {
        let response = router(
            true,
            true,
            Ok(Some(sample_binding(1, "power-system-1"))),
            Arc::default(),
        )
        .oneshot(request(request_path, Some(TOKEN)))
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        assert_published_example(&body, contract_path);
    }
}

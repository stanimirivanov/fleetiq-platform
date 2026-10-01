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
    ActorId, DefinitionName, ExternalIdentity, PropertyDefinition, PropertyDefinitionId,
    PropertyDefinitionRef, RelationshipTypeDefinition, RelationshipTypeRef, UnitId,
};
use tower::ServiceExt;

use super::*;

const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PATH: &str = "/api/v1/tenants/tenant-a/asset-types/generic.power-system/versions/2";
const EXPECTED_JSON: &str = r#"{"id":"generic.power-system","version":2,"name":"Power system","supported_properties":[{"id":"thermal.temperature","version":1},{"id":"electrical.voltage","version":3}]}"#;
const PROPERTY_PATH: &str = "/api/v1/tenants/tenant-a/properties/thermal.temperature/versions/1";
const EXPECTED_PROPERTY_JSON: &str = r#"{"id":"thermal.temperature","version":1,"name":"Air temperature","value_kind":"decimal","canonical_unit":"Cel"}"#;

#[derive(Clone, Copy, Debug)]
struct TestFailure;

impl std::fmt::Display for TestFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("private definition adapter detail")
    }
}

impl Error for TestFailure {}

struct TestResolver;

impl ActorIdentityResolver for TestResolver {
    type Error = TestFailure;

    fn resolve<'a>(
        &'a self,
        identity: &'a ExternalIdentity,
    ) -> impl Future<Output = Result<Option<ActorId>, Self::Error>> + Send + 'a {
        assert_eq!(identity.authority_id().as_str(), "workload");
        assert_eq!(identity.subject().as_str(), "reader");
        ready(Ok(Some(ActorId::new("actor-a").unwrap())))
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

struct TestDefinitions {
    result: Result<Option<AssetTypeDefinition>, TestFailure>,
    property_result: Result<Option<PropertyDefinition>, TestFailure>,
    calls: Arc<AtomicUsize>,
}

impl DefinitionRepository for TestDefinitions {
    type Error = TestFailure;

    fn insert_property<'a>(
        &'a self,
        _: &'a TenantId,
        _: &'a PropertyDefinition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        ready(Ok(()))
    }

    fn find_property<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a PropertyDefinitionRef,
    ) -> impl Future<Output = Result<Option<PropertyDefinition>, Self::Error>> + Send + 'a {
        assert_eq!(tenant_id.as_str(), "tenant-a");
        assert_eq!(reference.id().as_str(), "thermal.temperature");
        assert_eq!(reference.version().get(), 1);
        self.calls.fetch_add(1, Ordering::SeqCst);
        ready(self.property_result.clone())
    }

    fn insert_asset_type<'a>(
        &'a self,
        _: &'a TenantId,
        _: &'a AssetTypeDefinition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        ready(Ok(()))
    }

    fn find_asset_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a AssetTypeRef,
    ) -> impl Future<Output = Result<Option<AssetTypeDefinition>, Self::Error>> + Send + 'a {
        assert_eq!(tenant_id.as_str(), "tenant-a");
        assert_eq!(reference.id().as_str(), "generic.power-system");
        assert_eq!(reference.version().get(), 2);
        self.calls.fetch_add(1, Ordering::SeqCst);
        ready(self.result.clone())
    }

    fn insert_relationship_type<'a>(
        &'a self,
        _: &'a TenantId,
        _: &'a RelationshipTypeDefinition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        ready(Ok(()))
    }

    fn find_relationship_type<'a>(
        &'a self,
        _: &'a TenantId,
        _: &'a RelationshipTypeRef,
    ) -> impl Future<Output = Result<Option<RelationshipTypeDefinition>, Self::Error>> + Send + 'a
    {
        ready(Ok(None))
    }
}

fn sample_definition() -> AssetTypeDefinition {
    let mut definition = AssetTypeDefinition::new(
        AssetTypeRef::new(
            AssetTypeId::new("generic.power-system").unwrap(),
            DefinitionVersion::new(2).unwrap(),
        ),
        DefinitionName::new("Power system").unwrap(),
    );
    for (id, version) in [("thermal.temperature", 1), ("electrical.voltage", 3)] {
        definition
            .add_property(PropertyDefinitionRef::new(
                PropertyDefinitionId::new(id).unwrap(),
                DefinitionVersion::new(version).unwrap(),
            ))
            .unwrap();
    }
    definition
}

fn sample_property(value_kind: PropertyValueKind, unit: Option<&str>) -> PropertyDefinition {
    PropertyDefinition::new(
        PropertyDefinitionRef::new(
            PropertyDefinitionId::new("thermal.temperature").unwrap(),
            DefinitionVersion::new(1).unwrap(),
        ),
        DefinitionName::new("Air temperature").unwrap(),
        value_kind,
        unit.map(|unit| UnitId::new(unit).unwrap()),
    )
    .unwrap()
}

fn router(
    allowed: bool,
    result: Result<Option<AssetTypeDefinition>, TestFailure>,
    calls: Arc<AtomicUsize>,
) -> Router {
    router_with_results(allowed, result, Ok(None), calls)
}

fn property_router(
    allowed: bool,
    result: Result<Option<PropertyDefinition>, TestFailure>,
    calls: Arc<AtomicUsize>,
) -> Router {
    router_with_results(allowed, Ok(None), result, calls)
}

fn router_with_results(
    allowed: bool,
    result: Result<Option<AssetTypeDefinition>, TestFailure>,
    property_result: Result<Option<PropertyDefinition>, TestFailure>,
    calls: Arc<AtomicUsize>,
) -> Router {
    let bearer = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "reader")
        .unwrap()
        .bearer_identity()
        .unwrap()
        .clone();
    definition_router(
        CallerAuthenticationService::new(ConfiguredBearerAuthenticator::new(bearer), TestResolver),
        TenantAuthorizationService::new(TestPolicy { allowed }),
        DefinitionApplicationService::new(TestDefinitions {
            result,
            property_result,
            calls,
        }),
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
async fn authorized_query_returns_pinned_properties_in_declaration_order() {
    let calls = Arc::new(AtomicUsize::new(0));
    let response = router(true, Ok(Some(sample_definition())), calls.clone())
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
async fn credential_and_tenant_policy_failures_prevent_definition_lookup() {
    let calls = Arc::new(AtomicUsize::new(0));
    for (allowed, token, path, expected) in [
        (true, None, PATH, StatusCode::UNAUTHORIZED),
        (
            true,
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            PATH,
            StatusCode::UNAUTHORIZED,
        ),
        (false, Some(TOKEN), PATH, StatusCode::FORBIDDEN),
        (
            true,
            Some(TOKEN),
            "/api/v1/tenants/tenant-b/asset-types/generic.power-system/versions/2",
            StatusCode::FORBIDDEN,
        ),
    ] {
        let response = router(allowed, Ok(Some(sample_definition())), calls.clone())
            .oneshot(request(path, token))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn invalid_version_and_missing_definition_do_not_fall_back_to_latest() {
    let calls = Arc::new(AtomicUsize::new(0));
    let router = router(true, Ok(None), calls.clone());
    for version in ["0", "not-a-version", "4294967296", "+2"] {
        let path =
            format!("/api/v1/tenants/tenant-a/asset-types/generic.power-system/versions/{version}");
        let response = router
            .clone()
            .oneshot(request(&path, Some(TOKEN)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
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
async fn definition_failure_is_redacted_and_other_methods_are_rejected() {
    let failed = router(true, Err(TestFailure), Arc::new(AtomicUsize::new(0)));
    let response = failed.oneshot(request(PATH, Some(TOKEN))).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        !String::from_utf8_lossy(&to_bytes(response.into_body(), 1024).await.unwrap())
            .contains("private definition adapter detail")
    );

    let router = router(
        true,
        Ok(Some(sample_definition())),
        Arc::new(AtomicUsize::new(0)),
    );
    let response = router
        .oneshot(Request::post(PATH).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn authorized_property_query_returns_exact_meaning_and_unit() {
    let calls = Arc::new(AtomicUsize::new(0));
    let response = property_router(
        true,
        Ok(Some(sample_property(
            PropertyValueKind::Decimal,
            Some("Cel"),
        ))),
        calls.clone(),
    )
    .oneshot(request(PROPERTY_PATH, Some(TOKEN)))
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        to_bytes(response.into_body(), 2048).await.unwrap(),
        EXPECTED_PROPERTY_JSON
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn property_value_kinds_and_absent_unit_have_stable_wire_names() {
    for (kind, wire_name) in [
        (PropertyValueKind::Boolean, "boolean"),
        (PropertyValueKind::Integer, "integer"),
        (PropertyValueKind::Decimal, "decimal"),
        (PropertyValueKind::Text, "text"),
    ] {
        let response = property_router(
            true,
            Ok(Some(sample_property(kind, None))),
            Arc::new(AtomicUsize::new(0)),
        )
        .oneshot(request(PROPERTY_PATH, Some(TOKEN)))
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = String::from_utf8(to_bytes(response.into_body(), 2048).await.unwrap().to_vec())
            .unwrap();
        assert!(body.contains(&format!("\"value_kind\":\"{wire_name}\"")));
        assert!(body.contains("\"canonical_unit\":null"));
    }
}

#[tokio::test]
async fn property_query_requires_credentials_and_current_tenant_grant() {
    let calls = Arc::new(AtomicUsize::new(0));
    for (allowed, token, path, expected) in [
        (true, None, PROPERTY_PATH, StatusCode::UNAUTHORIZED),
        (
            true,
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            PROPERTY_PATH,
            StatusCode::UNAUTHORIZED,
        ),
        (false, Some(TOKEN), PROPERTY_PATH, StatusCode::FORBIDDEN),
        (
            true,
            Some(TOKEN),
            "/api/v1/tenants/tenant-b/properties/thermal.temperature/versions/1",
            StatusCode::FORBIDDEN,
        ),
    ] {
        let response = property_router(
            allowed,
            Ok(Some(sample_property(
                PropertyValueKind::Decimal,
                Some("Cel"),
            ))),
            calls.clone(),
        )
        .oneshot(request(path, token))
        .await
        .unwrap();
        assert_eq!(response.status(), expected);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn property_query_rejects_invalid_paths_and_does_not_choose_another_version() {
    let calls = Arc::new(AtomicUsize::new(0));
    let router = property_router(true, Ok(None), calls.clone());
    for path in [
        "/api/v1/tenants/tenant-a/properties/thermal.temperature/versions/0",
        "/api/v1/tenants/tenant-a/properties/thermal.temperature/versions/nope",
        "/api/v1/tenants/tenant-a/properties/thermal.temperature/versions/4294967296",
        "/api/v1/tenants/tenant-a/properties/thermal.temperature/versions/+1",
        "/api/v1/tenants/tenant-a/properties/%20bad/versions/1",
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
            .oneshot(request(PROPERTY_PATH, Some(TOKEN)))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn property_repository_failures_are_redacted_and_other_methods_are_rejected() {
    let response = property_router(true, Err(TestFailure), Arc::new(AtomicUsize::new(0)))
        .oneshot(request(PROPERTY_PATH, Some(TOKEN)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        !String::from_utf8_lossy(&to_bytes(response.into_body(), 1024).await.unwrap())
            .contains("private definition adapter detail")
    );

    let response = property_router(
        true,
        Ok(Some(sample_property(
            PropertyValueKind::Decimal,
            Some("Cel"),
        ))),
        Arc::new(AtomicUsize::new(0)),
    )
    .oneshot(Request::post(PROPERTY_PATH).body(Body::empty()).unwrap())
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn configured_asset_type_query_uses_exact_version_and_current_postgres_grant(
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

    let repository = PostgresDefinitionRepository::new(pool.clone());
    let tenant = TenantId::new("tenant-a")?;
    let definition = sample_definition();
    for reference in definition.supported_properties() {
        let property = PropertyDefinition::new(
            reference.clone(),
            DefinitionName::new(reference.id().as_str())?,
            PropertyValueKind::Decimal,
            None,
        )?;
        repository.insert_property(&tenant, &property).await?;
    }
    repository.insert_asset_type(&tenant, &definition).await?;

    let bearer = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")?
        .with_bearer_identity(TOKEN, "workload", "reader")?
        .bearer_identity()
        .expect("configured credential")
        .clone();
    let router = postgres_definition_router(pool.clone(), bearer);
    let response = router.clone().oneshot(request(PATH, Some(TOKEN))).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 2048).await?, EXPECTED_JSON);
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/api/v1/tenants/tenant-a/asset-types/generic.power-system/versions/1",
                Some(TOKEN)
            ))
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/api/v1/tenants/tenant-b/asset-types/generic.power-system/versions/2",
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

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn configured_property_query_reads_pinned_version_and_current_postgres_grant(
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

    let repository = PostgresDefinitionRepository::new(pool.clone());
    let tenant = TenantId::new("tenant-a")?;
    repository
        .insert_property(
            &tenant,
            &sample_property(PropertyValueKind::Decimal, Some("Cel")),
        )
        .await?;
    repository
        .insert_property(
            &tenant,
            &PropertyDefinition::new(
                PropertyDefinitionRef::new(
                    PropertyDefinitionId::new("thermal.temperature")?,
                    DefinitionVersion::new(2)?,
                ),
                DefinitionName::new("Revised temperature")?,
                PropertyValueKind::Integer,
                None,
            )?,
        )
        .await?;

    let bearer = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")?
        .with_bearer_identity(TOKEN, "workload", "reader")?
        .bearer_identity()
        .expect("configured credential")
        .clone();
    let router = postgres_definition_router(pool.clone(), bearer);
    let response = router
        .clone()
        .oneshot(request(PROPERTY_PATH, Some(TOKEN)))
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 2048).await?,
        EXPECTED_PROPERTY_JSON
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/api/v1/tenants/tenant-a/properties/thermal.temperature/versions/3",
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
                "/api/v1/tenants/tenant-b/properties/thermal.temperature/versions/1",
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
        router
            .oneshot(request(PROPERTY_PATH, Some(TOKEN)))
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    Ok(())
}

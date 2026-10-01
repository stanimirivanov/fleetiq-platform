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
    PropertyDefinitionRef, PropertyValueKind, RelationshipTypeDefinition, RelationshipTypeRef,
};
use tower::ServiceExt;

use super::*;

const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PATH: &str = "/api/v1/tenants/tenant-a/asset-types/generic.power-system/versions/2";
const EXPECTED_JSON: &str = r#"{"id":"generic.power-system","version":2,"name":"Power system","supported_properties":[{"id":"thermal.temperature","version":1},{"id":"electrical.voltage","version":3}]}"#;

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
        _: &'a TenantId,
        _: &'a PropertyDefinitionRef,
    ) -> impl Future<Output = Result<Option<PropertyDefinition>, Self::Error>> + Send + 'a {
        ready(Ok(None))
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

fn router(
    allowed: bool,
    result: Result<Option<AssetTypeDefinition>, TestFailure>,
    calls: Arc<AtomicUsize>,
) -> Router {
    let bearer = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "reader")
        .unwrap()
        .bearer_identity()
        .unwrap()
        .clone();
    asset_type_router(
        CallerAuthenticationService::new(ConfiguredBearerAuthenticator::new(bearer), TestResolver),
        TenantAuthorizationService::new(TestPolicy { allowed }),
        DefinitionApplicationService::new(TestDefinitions { result, calls }),
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
    let router = postgres_asset_type_router(pool.clone(), bearer);
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

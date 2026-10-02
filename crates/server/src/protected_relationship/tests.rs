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
    ActorId, Asset, AssetId, AssetName, AssetTypeDefinition, AssetTypeId, AssetTypeRef,
    DefinitionName, DefinitionVersion, EffectiveInterval, ExternalIdentity, RecordRevision,
    RecordedRevision, RelationshipEndpoints, RelationshipTypeDefinition, RelationshipTypeId,
    RelationshipTypeRef, UnixTimeMillis,
};
use fleetiq_metadata::{AssetRepository, DefinitionRepository, RelationshipSnapshotQuery};
use fleetiq_postgres::{PostgresAssetRepository, PostgresDefinitionRepository};
use tower::ServiceExt;

use super::*;

const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PATH: &str = "/api/v1/tenants/tenant-a/relationships/relationship-1/revisions/1";
const EXPECTED_JSON: &str = r#"{"id":"relationship-1","tenant_id":"tenant-a","revision":"1","recorded_at_ms":2000,"relationship_type":{"id":"physical.contains","version":1},"source_asset_id":"assembly-a","target_asset_id":"component-b","effective_interval":{"start_ms":1000,"end_ms":3000}}"#;

#[derive(Clone, Copy, Debug)]
struct TestFailure;

impl std::fmt::Display for TestFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("private relationship adapter detail")
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

struct TestRelationships {
    result: Result<Option<AssetRelationship>, TestFailure>,
    calls: Arc<AtomicUsize>,
}

impl RelationshipRepository for TestRelationships {
    type Error = TestFailure;

    fn insert_revision(
        &self,
        _: &AssetRelationship,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        ready(Ok(()))
    }

    fn find_revision(
        &self,
        tenant_id: &TenantId,
        relationship_id: &AssetRelationshipId,
        revision: RecordRevision,
    ) -> impl Future<Output = Result<Option<AssetRelationship>, Self::Error>> + Send {
        assert_eq!(tenant_id.as_str(), "tenant-a");
        assert_eq!(relationship_id.as_str(), "relationship-1");
        assert_eq!(revision.get(), 1);
        self.calls.fetch_add(1, Ordering::SeqCst);
        ready(self.result.clone())
    }

    fn find_for_asset(
        &self,
        _: &TenantId,
        _: &AssetId,
        _: RelationshipSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<AssetRelationship>, Self::Error>> + Send {
        ready(Ok(Vec::new()))
    }
}

fn type_reference() -> RelationshipTypeRef {
    RelationshipTypeRef::new(
        RelationshipTypeId::new("physical.contains").unwrap(),
        DefinitionVersion::new(1).unwrap(),
    )
}

fn sample_relationship(revision: u64, target: &str) -> AssetRelationship {
    AssetRelationship::new(
        TenantId::new("tenant-a").unwrap(),
        AssetRelationshipId::new("relationship-1").unwrap(),
        RecordedRevision::new(
            RecordRevision::new(revision).unwrap(),
            UnixTimeMillis::new(if revision == 1 { 2_000 } else { 4_000 }),
        ),
        type_reference(),
        RelationshipEndpoints::new(
            AssetId::new("assembly-a").unwrap(),
            AssetId::new(target).unwrap(),
        ),
        EffectiveInterval::new(
            UnixTimeMillis::new(1_000),
            (revision == 1).then_some(UnixTimeMillis::new(3_000)),
        )
        .unwrap(),
    )
}

fn router(
    allowed: bool,
    result: Result<Option<AssetRelationship>, TestFailure>,
    calls: Arc<AtomicUsize>,
) -> Router {
    let bearer = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "reader")
        .unwrap()
        .bearer_identity()
        .unwrap()
        .clone();
    relationship_router(
        CallerAuthenticationService::new(ConfiguredBearerAuthenticator::new(bearer), TestResolver),
        TenantAuthorizationService::new(TestPolicy { allowed }),
        RelationshipApplicationService::new(TestRelationships { result, calls }),
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
async fn authorized_query_returns_exact_recorded_and_effective_meaning() {
    let calls = Arc::new(AtomicUsize::new(0));
    let response = router(
        true,
        Ok(Some(sample_relationship(1, "component-b"))),
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
async fn authentication_and_exact_grant_prevent_unauthorized_lookup() {
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
            "/api/v1/tenants/tenant-b/relationships/relationship-1/revisions/1",
            StatusCode::FORBIDDEN,
        ),
    ] {
        let response = router(
            allowed,
            Ok(Some(sample_relationship(1, "component-b"))),
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
async fn invalid_path_and_absent_exact_revision_have_bounded_results() {
    let calls = Arc::new(AtomicUsize::new(0));
    let router = router(true, Ok(None), calls.clone());
    for path in [
        "/api/v1/tenants/tenant-a/relationships/relationship-1/revisions/0",
        "/api/v1/tenants/tenant-a/relationships/relationship-1/revisions/+1",
        "/api/v1/tenants/tenant-a/relationships/relationship-1/revisions/nope",
        "/api/v1/tenants/tenant-a/relationships/relationship-1/revisions/18446744073709551616",
        "/api/v1/tenants/tenant-a/relationships/%20bad/revisions/1",
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
async fn operational_error_is_redacted_and_other_methods_are_rejected() {
    let response = router(true, Err(TestFailure), Arc::default())
        .oneshot(request(PATH, Some(TOKEN)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        !String::from_utf8_lossy(&to_bytes(response.into_body(), 1024).await.unwrap())
            .contains("private relationship adapter detail")
    );

    let response = router(true, Ok(None), Arc::default())
        .oneshot(Request::post(PATH).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn configured_query_reads_exact_revision_with_current_postgres_grant(
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

    let tenant = TenantId::new("tenant-a")?;
    let asset_type = AssetTypeRef::new(
        AssetTypeId::new("generic.machine")?,
        DefinitionVersion::new(1)?,
    );
    let definitions = PostgresDefinitionRepository::new(pool.clone());
    definitions
        .insert_asset_type(
            &tenant,
            &AssetTypeDefinition::new(asset_type.clone(), DefinitionName::new("Machine")?),
        )
        .await?;
    definitions
        .insert_relationship_type(
            &tenant,
            &RelationshipTypeDefinition::new(type_reference(), DefinitionName::new("Contains")?),
        )
        .await?;
    let assets = PostgresAssetRepository::new(pool.clone());
    for id in ["assembly-a", "component-b", "component-c"] {
        assets
            .insert(&Asset::new(
                tenant.clone(),
                AssetId::new(id)?,
                asset_type.clone(),
                AssetName::new(id)?,
            ))
            .await?;
    }
    let relationships = PostgresRelationshipRepository::new(pool.clone());
    relationships
        .insert_revision(&sample_relationship(1, "component-b"))
        .await?;
    relationships
        .insert_revision(&sample_relationship(2, "component-c"))
        .await?;

    let bearer = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")?
        .with_bearer_identity(TOKEN, "workload", "reader")?
        .bearer_identity()
        .expect("configured credential")
        .clone();
    let router = postgres_relationship_router(pool.clone(), bearer);
    let response = router.clone().oneshot(request(PATH, Some(TOKEN))).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 2048).await?, EXPECTED_JSON);

    let response = router
        .clone()
        .oneshot(request(
            "/api/v1/tenants/tenant-a/relationships/relationship-1/revisions/2",
            Some(TOKEN),
        ))
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 2048).await?;
    assert!(String::from_utf8_lossy(&body).contains("\"target_asset_id\":\"component-c\""));
    assert!(String::from_utf8_lossy(&body).contains("\"revision\":\"2\""));
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/api/v1/tenants/tenant-a/relationships/relationship-1/revisions/3",
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
                "/api/v1/tenants/tenant-b/relationships/relationship-1/revisions/1",
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

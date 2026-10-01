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
    relationship_result: Result<Option<RelationshipTypeDefinition>, TestFailure>,
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
        tenant_id: &'a TenantId,
        reference: &'a RelationshipTypeRef,
    ) -> impl Future<Output = Result<Option<RelationshipTypeDefinition>, Self::Error>> + Send + 'a
    {
        assert_eq!(tenant_id.as_str(), "tenant-a");
        assert_eq!(reference.id().as_str(), "generic.tows");
        assert_eq!(reference.version().get(), 2);
        self.calls.fetch_add(1, Ordering::SeqCst);
        ready(self.relationship_result.clone())
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

fn sample_relationship_type() -> RelationshipTypeDefinition {
    RelationshipTypeDefinition::new(
        RelationshipTypeRef::new(
            RelationshipTypeId::new("generic.tows").unwrap(),
            DefinitionVersion::new(2).unwrap(),
        ),
        DefinitionName::new("Tows").unwrap(),
    )
}

fn router(
    allowed: bool,
    result: Result<Option<AssetTypeDefinition>, TestFailure>,
    calls: Arc<AtomicUsize>,
) -> Router {
    router_with_results(allowed, result, Ok(None), Ok(None), calls)
}

fn property_router(
    allowed: bool,
    result: Result<Option<PropertyDefinition>, TestFailure>,
    calls: Arc<AtomicUsize>,
) -> Router {
    router_with_results(allowed, Ok(None), result, Ok(None), calls)
}

fn relationship_type_router(
    allowed: bool,
    result: Result<Option<RelationshipTypeDefinition>, TestFailure>,
    calls: Arc<AtomicUsize>,
) -> Router {
    router_with_results(allowed, Ok(None), Ok(None), result, calls)
}

fn router_with_results(
    allowed: bool,
    result: Result<Option<AssetTypeDefinition>, TestFailure>,
    property_result: Result<Option<PropertyDefinition>, TestFailure>,
    relationship_result: Result<Option<RelationshipTypeDefinition>, TestFailure>,
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
            relationship_result,
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

mod asset_type;
mod property;
mod relationship_type;

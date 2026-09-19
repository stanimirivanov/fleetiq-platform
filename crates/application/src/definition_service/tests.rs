use std::{
    convert::Infallible,
    error::Error,
    fmt,
    future::{Future, ready},
    sync::{Arc, Mutex},
};

use fleetiq_domain::{
    ActorId, AssetTypeDefinition, AssetTypeId, AssetTypeRef, DefinitionName, DefinitionVersion,
    PropertyDefinition, PropertyDefinitionId, PropertyDefinitionRef, PropertyValueKind,
    RelationshipTypeDefinition, RelationshipTypeId, RelationshipTypeRef, TenantId, UnitId,
};

use crate::{
    AccessDecision, AuthorizedTenantAction, DefinitionRepository, TenantAccessPolicy,
    TenantAccessRequest, TenantAction, TenantAuthorizationService,
};

use super::{DefinitionApplicationError, DefinitionApplicationService};

#[derive(Clone, Debug, Eq, PartialEq)]
enum RepositoryCall {
    InsertProperty(TenantId, PropertyDefinition),
    FindProperty(TenantId, PropertyDefinitionRef),
    InsertAssetType(TenantId, AssetTypeDefinition),
    FindAssetType(TenantId, AssetTypeRef),
    InsertRelationshipType(TenantId, RelationshipTypeDefinition),
    FindRelationshipType(TenantId, RelationshipTypeRef),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TestRepositoryError;

impl fmt::Display for TestRepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("test repository failed")
    }
}

impl Error for TestRepositoryError {}

#[derive(Clone, Debug, Default)]
struct RecordingDefinitionRepository {
    calls: Arc<Mutex<Vec<RepositoryCall>>>,
    fail: bool,
    property: Option<PropertyDefinition>,
    asset_type: Option<AssetTypeDefinition>,
    relationship_type: Option<RelationshipTypeDefinition>,
}

impl RecordingDefinitionRepository {
    fn returning(
        property: PropertyDefinition,
        asset_type: AssetTypeDefinition,
        relationship_type: RelationshipTypeDefinition,
    ) -> Self {
        Self {
            property: Some(property),
            asset_type: Some(asset_type),
            relationship_type: Some(relationship_type),
            ..Self::default()
        }
    }

    fn failing() -> Self {
        Self {
            fail: true,
            ..Self::default()
        }
    }

    fn calls(&self) -> Vec<RepositoryCall> {
        self.calls.lock().expect("calls lock poisoned").clone()
    }

    fn record(&self, call: RepositoryCall) {
        self.calls.lock().expect("calls lock poisoned").push(call);
    }

    fn write_result(&self) -> Result<(), TestRepositoryError> {
        if self.fail {
            Err(TestRepositoryError)
        } else {
            Ok(())
        }
    }

    fn read_result<T: Clone>(&self, value: Option<&T>) -> Result<Option<T>, TestRepositoryError> {
        if self.fail {
            Err(TestRepositoryError)
        } else {
            Ok(value.cloned())
        }
    }
}

impl DefinitionRepository for RecordingDefinitionRepository {
    type Error = TestRepositoryError;

    fn insert_property<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        definition: &'a PropertyDefinition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        self.record(RepositoryCall::InsertProperty(
            tenant_id.clone(),
            definition.clone(),
        ));
        ready(self.write_result())
    }

    fn find_property<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a PropertyDefinitionRef,
    ) -> impl Future<Output = Result<Option<PropertyDefinition>, Self::Error>> + Send + 'a {
        self.record(RepositoryCall::FindProperty(
            tenant_id.clone(),
            reference.clone(),
        ));
        ready(self.read_result(self.property.as_ref()))
    }

    fn insert_asset_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        definition: &'a AssetTypeDefinition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        self.record(RepositoryCall::InsertAssetType(
            tenant_id.clone(),
            definition.clone(),
        ));
        ready(self.write_result())
    }

    fn find_asset_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a AssetTypeRef,
    ) -> impl Future<Output = Result<Option<AssetTypeDefinition>, Self::Error>> + Send + 'a {
        self.record(RepositoryCall::FindAssetType(
            tenant_id.clone(),
            reference.clone(),
        ));
        ready(self.read_result(self.asset_type.as_ref()))
    }

    fn insert_relationship_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        definition: &'a RelationshipTypeDefinition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        self.record(RepositoryCall::InsertRelationshipType(
            tenant_id.clone(),
            definition.clone(),
        ));
        ready(self.write_result())
    }

    fn find_relationship_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a RelationshipTypeRef,
    ) -> impl Future<Output = Result<Option<RelationshipTypeDefinition>, Self::Error>> + Send + 'a
    {
        self.record(RepositoryCall::FindRelationshipType(
            tenant_id.clone(),
            reference.clone(),
        ));
        ready(self.read_result(self.relationship_type.as_ref()))
    }
}

struct AllowAllPolicy;

impl TenantAccessPolicy for AllowAllPolicy {
    type Error = Infallible;

    fn evaluate<'a>(
        &'a self,
        _request: &'a TenantAccessRequest,
    ) -> impl Future<Output = Result<AccessDecision, Self::Error>> + Send + 'a {
        ready(Ok(AccessDecision::Allow))
    }
}

#[tokio::test]
async fn publishes_every_definition_kind_in_the_authorized_tenant() {
    let repository = RecordingDefinitionRepository::default();
    let service = DefinitionApplicationService::new(repository.clone());
    let property = property_definition();
    let asset_type = asset_type_definition();
    let relationship_type = relationship_type_definition();

    service
        .publish_property(manage_authorization().await, &property)
        .await
        .expect("property publication should succeed");
    service
        .publish_asset_type(manage_authorization().await, &asset_type)
        .await
        .expect("asset type publication should succeed");
    service
        .publish_relationship_type(manage_authorization().await, &relationship_type)
        .await
        .expect("relationship type publication should succeed");

    assert_eq!(
        repository.calls(),
        vec![
            RepositoryCall::InsertProperty(tenant_id(), property),
            RepositoryCall::InsertAssetType(tenant_id(), asset_type),
            RepositoryCall::InsertRelationshipType(tenant_id(), relationship_type),
        ]
    );
}

#[tokio::test]
async fn finds_every_exact_definition_kind_in_the_authorized_tenant() {
    let property = property_definition();
    let asset_type = asset_type_definition();
    let relationship_type = relationship_type_definition();
    let repository = RecordingDefinitionRepository::returning(
        property.clone(),
        asset_type.clone(),
        relationship_type.clone(),
    );
    let service = DefinitionApplicationService::new(repository.clone());

    assert_eq!(
        service
            .find_property(read_authorization().await, property.reference())
            .await
            .expect("property lookup should succeed"),
        Some(property.clone())
    );
    assert_eq!(
        service
            .find_asset_type(read_authorization().await, asset_type.reference())
            .await
            .expect("asset type lookup should succeed"),
        Some(asset_type.clone())
    );
    assert_eq!(
        service
            .find_relationship_type(read_authorization().await, relationship_type.reference())
            .await
            .expect("relationship type lookup should succeed"),
        Some(relationship_type.clone())
    );

    assert_eq!(
        repository.calls(),
        vec![
            RepositoryCall::FindProperty(tenant_id(), property.reference().clone()),
            RepositoryCall::FindAssetType(tenant_id(), asset_type.reference().clone()),
            RepositoryCall::FindRelationshipType(
                tenant_id(),
                relationship_type.reference().clone(),
            ),
        ]
    );
}

#[tokio::test]
async fn rejects_wrong_actions_before_calling_the_repository() {
    let repository = RecordingDefinitionRepository::default();
    let service = DefinitionApplicationService::new(repository.clone());

    let publication_error = service
        .publish_property(read_authorization().await, &property_definition())
        .await
        .expect_err("read authorization must not permit publication");
    assert!(matches!(
        publication_error,
        DefinitionApplicationError::ActionRequired {
            required: TenantAction::ManageMetadata,
            actual: TenantAction::ReadMetadata,
        }
    ));

    let lookup_error = service
        .find_asset_type(
            manage_authorization().await,
            asset_type_definition().reference(),
        )
        .await
        .expect_err("manage authorization must not imply read authorization");
    assert!(matches!(
        lookup_error,
        DefinitionApplicationError::ActionRequired {
            required: TenantAction::ReadMetadata,
            actual: TenantAction::ManageMetadata,
        }
    ));
    assert!(repository.calls().is_empty());
}

#[tokio::test]
async fn preserves_repository_failure_as_the_error_source() {
    let service = DefinitionApplicationService::new(RecordingDefinitionRepository::failing());

    let error = service
        .publish_relationship_type(
            manage_authorization().await,
            &relationship_type_definition(),
        )
        .await
        .expect_err("repository failure should be returned");

    assert!(matches!(
        error,
        DefinitionApplicationError::Repository(TestRepositoryError)
    ));
    assert!(error.source().is_some());
}

async fn manage_authorization() -> AuthorizedTenantAction {
    authorization(TenantAction::ManageMetadata).await
}

async fn read_authorization() -> AuthorizedTenantAction {
    authorization(TenantAction::ReadMetadata).await
}

async fn authorization(action: TenantAction) -> AuthorizedTenantAction {
    TenantAuthorizationService::new(AllowAllPolicy)
        .authorize(TenantAccessRequest::new(
            ActorId::new("actor-a").expect("valid actor ID"),
            tenant_id(),
            action,
        ))
        .await
        .expect("test policy should authorize request")
}

fn property_definition() -> PropertyDefinition {
    PropertyDefinition::new(
        PropertyDefinitionRef::new(
            PropertyDefinitionId::new("electrical.voltage").expect("valid property ID"),
            definition_version(),
        ),
        DefinitionName::new("Voltage").expect("valid definition name"),
        PropertyValueKind::Decimal,
        Some(UnitId::new("V").expect("valid unit ID")),
    )
    .expect("valid property definition")
}

fn asset_type_definition() -> AssetTypeDefinition {
    AssetTypeDefinition::new(
        AssetTypeRef::new(
            AssetTypeId::new("electrical.machine").expect("valid asset type ID"),
            definition_version(),
        ),
        DefinitionName::new("Electrical machine").expect("valid definition name"),
    )
}

fn relationship_type_definition() -> RelationshipTypeDefinition {
    RelationshipTypeDefinition::new(
        RelationshipTypeRef::new(
            RelationshipTypeId::new("physical.contains").expect("valid relationship type ID"),
            definition_version(),
        ),
        DefinitionName::new("Contains").expect("valid definition name"),
    )
}

fn definition_version() -> DefinitionVersion {
    DefinitionVersion::new(1).expect("valid definition version")
}

fn tenant_id() -> TenantId {
    TenantId::new("tenant-a").expect("valid tenant ID")
}

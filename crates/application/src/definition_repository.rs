use std::{error::Error, future::Future};

use fleetiq_domain::{
    AssetTypeDefinition, AssetTypeRef, PropertyDefinition, PropertyDefinitionRef,
    RelationshipTypeDefinition, RelationshipTypeRef, TenantId,
};

/// Persists and retrieves tenant-owned semantic definition versions.
///
/// Definition versions are immutable. A semantic change is inserted under a
/// new version and existing assets continue to reference their exact version.
/// The tenant remains an explicit argument because definition aggregates model
/// reusable meaning without owning infrastructure tenancy concerns.
pub trait DefinitionRepository: Send + Sync {
    /// The infrastructure-specific failure returned by this repository.
    type Error: Error + Send + Sync + 'static;

    /// Inserts one immutable property-definition version.
    fn insert_property<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        definition: &'a PropertyDefinition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a;

    /// Finds an exact property-definition version within a tenant.
    fn find_property<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a PropertyDefinitionRef,
    ) -> impl Future<Output = Result<Option<PropertyDefinition>, Self::Error>> + Send + 'a;

    /// Inserts one immutable asset-type version and its property references.
    fn insert_asset_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        definition: &'a AssetTypeDefinition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a;

    /// Finds an exact asset-type version within a tenant.
    fn find_asset_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a AssetTypeRef,
    ) -> impl Future<Output = Result<Option<AssetTypeDefinition>, Self::Error>> + Send + 'a;

    /// Inserts one immutable directed relationship-type version.
    fn insert_relationship_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        definition: &'a RelationshipTypeDefinition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a;

    /// Finds an exact relationship-type version within a tenant.
    fn find_relationship_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a RelationshipTypeRef,
    ) -> impl Future<Output = Result<Option<RelationshipTypeDefinition>, Self::Error>> + Send + 'a;
}

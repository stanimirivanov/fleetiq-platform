use std::future::Future;

use fleetiq_domain::{
    AssetTypeDefinition, AssetTypeRef, PropertyDefinition, PropertyDefinitionRef,
    RelationshipTypeDefinition, RelationshipTypeRef, TenantId,
};
use fleetiq_metadata::DefinitionRepository;
use fleetiq_semantics::PropertyDefinitionView;
use sqlx::PgPool;

use crate::error::PostgresRepositoryError;

mod asset_type;
mod common;
mod property;
mod relationship_type;

/// PostgreSQL implementation of the semantic-definition repository port.
#[derive(Clone, Debug)]
pub struct PostgresDefinitionRepository {
    pool: PgPool,
}

impl PostgresDefinitionRepository {
    /// Constructs a definition repository using an existing connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl DefinitionRepository for PostgresDefinitionRepository {
    type Error = PostgresRepositoryError;

    fn insert_property<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        definition: &'a PropertyDefinition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        self.insert_property_definition(tenant_id, definition)
    }

    fn find_property<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a PropertyDefinitionRef,
    ) -> impl Future<Output = Result<Option<PropertyDefinition>, Self::Error>> + Send + 'a {
        self.find_property_definition(tenant_id, reference)
    }

    fn insert_asset_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        definition: &'a AssetTypeDefinition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        self.insert_asset_type_definition(tenant_id, definition)
    }

    fn find_asset_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a AssetTypeRef,
    ) -> impl Future<Output = Result<Option<AssetTypeDefinition>, Self::Error>> + Send + 'a {
        self.find_asset_type_definition(tenant_id, reference)
    }

    fn insert_relationship_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        definition: &'a RelationshipTypeDefinition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        self.insert_relationship_type_definition(tenant_id, definition)
    }

    fn find_relationship_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a RelationshipTypeRef,
    ) -> impl Future<Output = Result<Option<RelationshipTypeDefinition>, Self::Error>> + Send + 'a
    {
        self.find_relationship_type_definition(tenant_id, reference)
    }
}

impl PropertyDefinitionView for PostgresDefinitionRepository {
    type Error = PostgresRepositoryError;

    fn find_property<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a PropertyDefinitionRef,
    ) -> impl Future<Output = Result<Option<PropertyDefinition>, Self::Error>> + Send + 'a {
        self.find_property_definition(tenant_id, reference)
    }
}

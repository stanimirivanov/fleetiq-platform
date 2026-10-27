use bigdecimal::BigDecimal;
use fleetiq_domain::{
    AssetId, AssetRelationship, AssetRelationshipId, DefinitionVersion, EffectiveInterval,
    RecordedRevision, RelationshipEndpoints, RelationshipTypeId, RelationshipTypeRef, TenantId,
    UnixTimeMillis,
};

use crate::error::{PostgresRepositoryError, corrupt};

use super::common::parse_revision;

#[derive(Debug)]
pub(in crate::metadata) struct RelationshipRevisionRow {
    pub(in crate::metadata) relationship_id: String,
    pub(in crate::metadata) revision: BigDecimal,
    pub(in crate::metadata) recorded_at_ms: i64,
    pub(in crate::metadata) relationship_type_id: String,
    pub(in crate::metadata) relationship_type_version: i64,
    pub(in crate::metadata) source_asset_id: String,
    pub(in crate::metadata) target_asset_id: String,
    pub(in crate::metadata) effective_start_ms: i64,
    pub(in crate::metadata) effective_end_ms: Option<i64>,
}

impl RelationshipRevisionRow {
    pub(in crate::metadata) fn into_domain(
        self,
        tenant_id: TenantId,
    ) -> Result<AssetRelationship, PostgresRepositoryError> {
        let interval = EffectiveInterval::new(
            UnixTimeMillis::new(self.effective_start_ms),
            self.effective_end_ms.map(UnixTimeMillis::new),
        )
        .map_err(|error| corrupt("asset_relationship_revisions.effective_interval", error))?;

        Ok(AssetRelationship::new(
            tenant_id,
            AssetRelationshipId::new(self.relationship_id)
                .map_err(|error| corrupt("asset_relationship_revisions.relationship_id", error))?,
            RecordedRevision::new(
                parse_revision("asset_relationship_revisions.revision", &self.revision)?,
                UnixTimeMillis::new(self.recorded_at_ms),
            ),
            RelationshipTypeRef::new(
                RelationshipTypeId::new(self.relationship_type_id).map_err(|error| {
                    corrupt("asset_relationship_revisions.relationship_type_id", error)
                })?,
                definition_version(
                    "asset_relationship_revisions.relationship_type_version",
                    self.relationship_type_version,
                )?,
            ),
            RelationshipEndpoints::new(
                AssetId::new(self.source_asset_id).map_err(|error| {
                    corrupt("asset_relationship_revisions.source_asset_id", error)
                })?,
                AssetId::new(self.target_asset_id).map_err(|error| {
                    corrupt("asset_relationship_revisions.target_asset_id", error)
                })?,
            ),
            interval,
        ))
    }
}

fn definition_version(
    field: &'static str,
    value: i64,
) -> Result<DefinitionVersion, PostgresRepositoryError> {
    let value = u32::try_from(value).map_err(|error| corrupt(field, error))?;
    DefinitionVersion::new(value).map_err(|error| corrupt(field, error))
}

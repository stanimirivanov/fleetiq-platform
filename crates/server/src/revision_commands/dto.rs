//! Closed transport values reconstruct validated domain facts with trusted time.
use fleetiq_domain::{
    AssetId, AssetRelationship, AssetRelationshipId, DefinitionVersion, DeviceEndpointId,
    EffectiveInterval, PropertyDefinitionId, PropertyDefinitionRef, RecordRevision,
    RecordedRevision, RelationshipEndpoints, RelationshipTypeId, RelationshipTypeRef,
    SignalBinding, SignalBindingId, SignalSource, SignalTarget, SourceSignalId, TenantId,
    UnixTimeMillis,
};
use fleetiq_metadata::MetadataRevision;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RelationshipBody {
    relationship_type: TypeRef,
    source_asset_id: String,
    target_asset_id: String,
    effective_interval: Interval,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypeRef {
    id: String,
    version: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Interval {
    start_ms: i64,
    end_ms: Option<i64>,
}
impl Interval {
    fn domain(self) -> Option<EffectiveInterval> {
        EffectiveInterval::new(
            UnixTimeMillis::new(self.start_ms),
            self.end_ms.map(UnixTimeMillis::new),
        )
        .ok()
    }
}
impl RelationshipBody {
    pub(super) fn domain(
        self,
        tenant: TenantId,
        id: String,
        revision: RecordRevision,
        time: UnixTimeMillis,
    ) -> Option<MetadataRevision> {
        Some(MetadataRevision::Relationship(AssetRelationship::new(
            tenant,
            AssetRelationshipId::new(id).ok()?,
            RecordedRevision::new(revision, time),
            RelationshipTypeRef::new(
                RelationshipTypeId::new(self.relationship_type.id).ok()?,
                DefinitionVersion::new(self.relationship_type.version).ok()?,
            ),
            RelationshipEndpoints::new(
                AssetId::new(self.source_asset_id).ok()?,
                AssetId::new(self.target_asset_id).ok()?,
            ),
            self.effective_interval.domain()?,
        )))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BindingBody {
    source: Source,
    target: Target,
    effective_interval: Interval,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    #[serde(rename = "device_asset_id")]
    device_asset: String,
    endpoint_id: String,
    signal_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    asset_id: String,
    property: TypeRef,
}
impl BindingBody {
    pub(super) fn domain(
        self,
        tenant: TenantId,
        id: String,
        revision: RecordRevision,
        time: UnixTimeMillis,
    ) -> Option<MetadataRevision> {
        Some(MetadataRevision::Binding(SignalBinding::new(
            tenant,
            SignalBindingId::new(id).ok()?,
            RecordedRevision::new(revision, time),
            SignalSource::new(
                AssetId::new(self.source.device_asset).ok()?,
                DeviceEndpointId::new(self.source.endpoint_id).ok()?,
                SourceSignalId::new(self.source.signal_id).ok()?,
            ),
            SignalTarget::new(
                AssetId::new(self.target.asset_id).ok()?,
                PropertyDefinitionRef::new(
                    PropertyDefinitionId::new(self.target.property.id).ok()?,
                    DefinitionVersion::new(self.target.property.version).ok()?,
                ),
            ),
            self.effective_interval.domain()?,
        )))
    }
}

//! HTTP input stays distinct from validated domain aggregates and catalogue scope.
use fleetiq_domain::{
    AssetTypeDefinition, AssetTypeRef, DefinitionName, DefinitionVersion, PropertyDefinition,
    PropertyDefinitionRef, PropertyValueKind, RelationshipTypeDefinition, RelationshipTypeRef,
    UnitId,
};
use fleetiq_metadata::{
    DefinitionCatalogueQuery, DefinitionIdentity, DefinitionKind, DefinitionPosition,
    SemanticDefinition,
};
use serde::{Deserialize, Serialize};

/// Untagged closed shapes retain duplicate-field rejection and route-owned kind.
#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum PublicationBody {
    Property(PropertyBody),
    AssetType(AssetTypeBody),
    RelationshipType(RelationshipTypeBody),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PropertyBody {
    id: String,
    version: u32,
    name: String,
    value_kind: ValueKind,
    canonical_unit: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AssetTypeBody {
    id: String,
    version: u32,
    name: String,
    supported_properties: Vec<PropertyReferenceBody>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RelationshipTypeBody {
    id: String,
    version: u32,
    name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertyReferenceBody {
    id: String,
    version: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ValueKind {
    Boolean,
    Integer,
    Decimal,
    Text,
}
impl From<ValueKind> for PropertyValueKind {
    fn from(value: ValueKind) -> Self {
        match value {
            ValueKind::Boolean => Self::Boolean,
            ValueKind::Integer => Self::Integer,
            ValueKind::Decimal => Self::Decimal,
            ValueKind::Text => Self::Text,
        }
    }
}
impl PublicationBody {
    pub(super) fn definition(self, kind: DefinitionKind) -> Option<SemanticDefinition> {
        match (kind, self) {
            (DefinitionKind::Property, Self::Property(body)) => Some(SemanticDefinition::Property(
                PropertyDefinition::new(
                    PropertyDefinitionRef::new(
                        fleetiq_domain::PropertyDefinitionId::new(body.id).ok()?,
                        DefinitionVersion::new(body.version).ok()?,
                    ),
                    DefinitionName::new(body.name).ok()?,
                    body.value_kind.into(),
                    body.canonical_unit.map(UnitId::new).transpose().ok()?,
                )
                .ok()?,
            )),
            (DefinitionKind::RelationshipType, Self::RelationshipType(body)) => Some(
                SemanticDefinition::RelationshipType(RelationshipTypeDefinition::new(
                    RelationshipTypeRef::new(
                        fleetiq_domain::RelationshipTypeId::new(body.id).ok()?,
                        DefinitionVersion::new(body.version).ok()?,
                    ),
                    DefinitionName::new(body.name).ok()?,
                )),
            ),
            (DefinitionKind::AssetType, Self::AssetType(body)) => {
                if body.supported_properties.len() > 100 {
                    return None;
                }
                let mut value = AssetTypeDefinition::new(
                    AssetTypeRef::new(
                        fleetiq_domain::AssetTypeId::new(body.id).ok()?,
                        DefinitionVersion::new(body.version).ok()?,
                    ),
                    DefinitionName::new(body.name).ok()?,
                );
                for property in body.supported_properties {
                    value
                        .add_property(PropertyDefinitionRef::new(
                            fleetiq_domain::PropertyDefinitionId::new(property.id).ok()?,
                            DefinitionVersion::new(property.version).ok()?,
                        ))
                        .ok()?;
                }
                Some(SemanticDefinition::AssetType(value))
            }
            _ => None,
        }
    }
}
#[derive(Serialize)]
pub(super) struct PublicationResponse {
    pub(super) outcome: &'static str,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CatalogueParameters {
    limit: Option<u8>,
    after_id: Option<String>,
    after_version: Option<u32>,
}
impl CatalogueParameters {
    pub(super) fn query(
        self,
        kind: DefinitionKind,
        identity: Option<String>,
    ) -> Option<DefinitionCatalogueQuery> {
        let identity = identity
            .map(|id| DefinitionIdentity::new(kind, id))
            .transpose()
            .ok()?;
        let after = match (self.after_id, self.after_version) {
            (None, None) => None,
            (Some(id), Some(version)) => Some(DefinitionPosition::new(
                DefinitionIdentity::new(kind, id).ok()?,
                DefinitionVersion::new(version).ok()?,
            )),
            _ => return None,
        };
        DefinitionCatalogueQuery::new(kind, self.limit.unwrap_or(50), identity, after).ok()
    }
}
#[derive(Serialize)]
pub(super) struct CatalogueEntry<'a> {
    pub(super) id: &'a str,
    pub(super) version: u32,
    pub(super) name: &'a str,
}
#[derive(Serialize)]
pub(super) struct CatalogueCursor<'a> {
    pub(super) id: &'a str,
    pub(super) version: u32,
}
#[derive(Serialize)]
pub(super) struct CatalogueResponse<'a> {
    pub(super) definitions: Vec<CatalogueEntry<'a>>,
    pub(super) next_after: Option<CatalogueCursor<'a>>,
}

//! Closed application vocabulary around existing domain definition aggregates.
use fleetiq_domain::{
    AssetTypeDefinition, AssetTypeId, DefinitionVersion, IdentifierError, PropertyDefinition,
    PropertyDefinitionId, RelationshipTypeDefinition, RelationshipTypeId,
};

/// Namespace of one immutable semantic definition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefinitionKind {
    /// Asset classification and ordered property membership.
    AssetType,
    /// Observable property meaning, representation and canonical unit.
    Property,
    /// Meaning of a directed graph edge.
    RelationshipType,
}

/// A validated identity retaining its definition namespace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DefinitionIdentity {
    /// Stable asset-type identity.
    AssetType(AssetTypeId),
    /// Stable property identity.
    Property(PropertyDefinitionId),
    /// Stable relationship-type identity.
    RelationshipType(RelationshipTypeId),
}
impl DefinitionIdentity {
    /// Validates an identifier in the selected namespace.
    ///
    /// # Errors
    ///
    /// Returns the domain identifier error for an invalid representation.
    pub fn new(kind: DefinitionKind, value: String) -> Result<Self, IdentifierError> {
        match kind {
            DefinitionKind::AssetType => AssetTypeId::new(value).map(Self::AssetType),
            DefinitionKind::Property => PropertyDefinitionId::new(value).map(Self::Property),
            DefinitionKind::RelationshipType => {
                RelationshipTypeId::new(value).map(Self::RelationshipType)
            }
        }
    }
    /// Returns the namespace; equal strings in different namespaces stay distinct.
    pub const fn kind(&self) -> DefinitionKind {
        match self {
            Self::AssetType(_) => DefinitionKind::AssetType,
            Self::Property(_) => DefinitionKind::Property,
            Self::RelationshipType(_) => DefinitionKind::RelationshipType,
        }
    }
    /// Returns the validated identifier text.
    pub fn as_str(&self) -> &str {
        match self {
            Self::AssetType(id) => id.as_str(),
            Self::Property(id) => id.as_str(),
            Self::RelationshipType(id) => id.as_str(),
        }
    }
}

/// One exact definition version, also usable as an exclusive catalogue position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DefinitionPosition {
    identity: DefinitionIdentity,
    version: DefinitionVersion,
}
impl DefinitionPosition {
    /// Creates a position from validated identity and positive version.
    pub const fn new(identity: DefinitionIdentity, version: DefinitionVersion) -> Self {
        Self { identity, version }
    }
    /// Returns the namespace-aware identity.
    pub const fn identity(&self) -> &DefinitionIdentity {
        &self.identity
    }
    /// Returns the exact numeric version.
    pub const fn version(&self) -> DefinitionVersion {
        self.version
    }
}

/// Validated immutable content submitted to the publication use case.
///
/// Asset-type property order is semantic content. Reordered references therefore
/// conflict under an occupied version even if their membership is identical.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SemanticDefinition {
    /// Complete asset-type aggregate.
    AssetType(AssetTypeDefinition),
    /// Complete property aggregate.
    Property(PropertyDefinition),
    /// Complete relationship-type aggregate.
    RelationshipType(RelationshipTypeDefinition),
}

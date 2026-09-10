use super::{DefinitionName, RelationshipTypeRef};

/// Defines the meaning of one immutable directed relationship type.
///
/// Endpoint type constraints, cardinality, and graph-shape rules require access
/// to definitions or relationships beyond this value. They belong to graph
/// validation rather than this definition aggregate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationshipTypeDefinition {
    reference: RelationshipTypeRef,
    name: DefinitionName,
}

impl RelationshipTypeDefinition {
    /// Constructs an immutable relationship-type definition.
    pub fn new(reference: RelationshipTypeRef, name: DefinitionName) -> Self {
        Self { reference, name }
    }

    /// Returns the identity and version of this immutable definition.
    pub fn reference(&self) -> &RelationshipTypeRef {
        &self.reference
    }

    /// Returns the operator-facing relationship-type name.
    pub fn name(&self) -> &DefinitionName {
        &self.name
    }
}

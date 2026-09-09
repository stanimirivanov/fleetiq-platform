use std::{error::Error, fmt};

use crate::{
    AssetId, AssetTypeRef, ExternalIdentifierAuthority, ExternalIdentifierKind,
    ExternalIdentifierValue, TenantId,
};

const MAX_ASSET_NAME_LENGTH: usize = 200;

/// A human-readable name used to distinguish an asset in operator workflows.
///
/// Names are labels rather than identities and do not need to be unique. `FleetIQ`
/// preserves the supplied text and rejects surrounding whitespace instead of
/// silently changing user input.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AssetName(String);

impl AssetName {
    /// Constructs a validated asset name.
    ///
    /// # Errors
    ///
    /// Returns [`AssetError`] when the name is empty, has surrounding
    /// whitespace, contains a control character, or exceeds 200 Unicode scalar
    /// values.
    pub fn new(value: impl Into<String>) -> Result<Self, AssetError> {
        let value = value.into();
        validate_name(&value)?;
        Ok(Self(value))
    }

    /// Returns the name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the name and returns its text representation.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for AssetName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for AssetName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// An identifier assigned to an asset by a system outside `FleetIQ`.
///
/// The kind states what the value means. The optional authority disambiguates
/// schemes whose values are unique only within a manufacturer, operator, or
/// another issuing system. Tenant scope comes from the owning [`Asset`].
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExternalIdentifier {
    kind: ExternalIdentifierKind,
    authority: Option<ExternalIdentifierAuthority>,
    value: ExternalIdentifierValue,
}

impl ExternalIdentifier {
    /// Constructs an external identifier.
    pub fn new(
        kind: ExternalIdentifierKind,
        authority: Option<ExternalIdentifierAuthority>,
        value: ExternalIdentifierValue,
    ) -> Self {
        Self {
            kind,
            authority,
            value,
        }
    }

    /// Returns the scheme that defines the identifier's meaning.
    pub fn kind(&self) -> &ExternalIdentifierKind {
        &self.kind
    }

    /// Returns the system that issued the identifier, when separately required.
    pub fn authority(&self) -> Option<&ExternalIdentifierAuthority> {
        self.authority.as_ref()
    }

    /// Returns the externally assigned value.
    pub fn value(&self) -> &ExternalIdentifierValue {
        &self.value
    }
}

/// A physical or logical thing managed by `FleetIQ`.
///
/// Asset meaning comes from its configurable asset type. This aggregate does not
/// assume that an asset is a vehicle: machines, infrastructure, subsystems,
/// sensors, and communication devices use the same model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Asset {
    tenant_id: TenantId,
    id: AssetId,
    type_reference: AssetTypeRef,
    name: AssetName,
    external_identifiers: Vec<ExternalIdentifier>,
}

impl Asset {
    /// Creates an asset without external identifiers.
    pub fn new(
        tenant_id: TenantId,
        id: AssetId,
        type_reference: AssetTypeRef,
        name: AssetName,
    ) -> Self {
        Self {
            tenant_id,
            id,
            type_reference,
            name,
            external_identifiers: Vec::new(),
        }
    }

    /// Returns the tenant that owns the asset.
    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the asset's `FleetIQ` identity.
    pub fn id(&self) -> &AssetId {
        &self.id
    }

    /// Returns the configurable type that gives the asset semantic meaning.
    pub fn asset_type(&self) -> &AssetTypeRef {
        &self.type_reference
    }

    /// Returns the current operator-facing name.
    pub fn name(&self) -> &AssetName {
        &self.name
    }

    /// Changes the operator-facing name.
    pub fn rename(&mut self, name: AssetName) {
        self.name = name;
    }

    /// Returns the identifiers assigned by external systems.
    pub fn external_identifiers(&self) -> &[ExternalIdentifier] {
        &self.external_identifiers
    }

    /// Adds an identifier when the same kind, authority, and value are not
    /// already attached to this asset.
    ///
    /// Different identifier kinds may intentionally use the same textual value.
    /// Uniqueness across assets is a repository invariant because it requires
    /// tenant-wide knowledge.
    ///
    /// # Errors
    ///
    /// Returns [`AssetError::DuplicateExternalIdentifier`] when an identical
    /// identifier is already attached.
    pub fn add_external_identifier(
        &mut self,
        identifier: ExternalIdentifier,
    ) -> Result<(), AssetError> {
        if self.external_identifiers.contains(&identifier) {
            return Err(AssetError::DuplicateExternalIdentifier);
        }

        self.external_identifiers.push(identifier);
        Ok(())
    }
}

/// Describes a rejected asset operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssetError {
    /// The supplied name is empty or contains only whitespace.
    EmptyName,
    /// The supplied name begins or ends with whitespace.
    NameHasSurroundingWhitespace,
    /// The supplied name exceeds the supported size.
    NameTooLong {
        /// The maximum accepted Unicode scalar-value count.
        maximum: usize,
        /// The Unicode scalar-value count in the rejected name.
        actual: usize,
    },
    /// The supplied name contains a control character.
    NameContainsControlCharacter,
    /// The exact external identifier is already attached to the asset.
    DuplicateExternalIdentifier,
}

impl fmt::Display for AssetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => formatter.write_str("asset name must not be empty"),
            Self::NameHasSurroundingWhitespace => {
                formatter.write_str("asset name must not have surrounding whitespace")
            }
            Self::NameTooLong { maximum, actual } => write!(
                formatter,
                "asset name contains {actual} characters; the maximum is {maximum}"
            ),
            Self::NameContainsControlCharacter => {
                formatter.write_str("asset name must not contain control characters")
            }
            Self::DuplicateExternalIdentifier => {
                formatter.write_str("external identifier is already attached to the asset")
            }
        }
    }
}

impl Error for AssetError {}

fn validate_name(value: &str) -> Result<(), AssetError> {
    if value.trim().is_empty() {
        return Err(AssetError::EmptyName);
    }
    if value.trim() != value {
        return Err(AssetError::NameHasSurroundingWhitespace);
    }
    if value.chars().any(char::is_control) {
        return Err(AssetError::NameContainsControlCharacter);
    }

    let actual = value.chars().count();
    if actual > MAX_ASSET_NAME_LENGTH {
        return Err(AssetError::NameTooLong {
            maximum: MAX_ASSET_NAME_LENGTH,
            actual,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssetTypeId, DefinitionVersion};

    fn asset() -> Asset {
        Asset::new(
            TenantId::new("tenant-a").expect("valid tenant ID"),
            AssetId::new("asset-1").expect("valid asset ID"),
            AssetTypeRef::new(
                AssetTypeId::new("industrial.machine").expect("valid asset type ID"),
                DefinitionVersion::new(1).expect("valid definition version"),
            ),
            AssetName::new("Primary machine").expect("valid asset name"),
        )
    }

    fn external_identifier(kind: &str, value: &str) -> ExternalIdentifier {
        ExternalIdentifier::new(
            ExternalIdentifierKind::new(kind).expect("valid identifier kind"),
            None,
            ExternalIdentifierValue::new(value).expect("valid identifier value"),
        )
    }

    #[test]
    fn creates_an_industry_neutral_asset() {
        let asset = asset();

        assert_eq!(asset.tenant_id().as_str(), "tenant-a");
        assert_eq!(asset.id().as_str(), "asset-1");
        assert_eq!(asset.asset_type().id().as_str(), "industrial.machine");
        assert_eq!(asset.asset_type().version().get(), 1);
        assert_eq!(asset.name().as_str(), "Primary machine");
        assert!(asset.external_identifiers().is_empty());
    }

    #[test]
    fn renames_an_asset_with_an_already_validated_name() {
        let mut asset = asset();

        asset.rename(AssetName::new("Backup machine").expect("valid asset name"));

        assert_eq!(asset.name().as_str(), "Backup machine");
    }

    #[test]
    fn rejects_invalid_names_without_normalizing_them() {
        assert_eq!(AssetName::new(""), Err(AssetError::EmptyName));
        assert_eq!(
            AssetName::new(" Machine"),
            Err(AssetError::NameHasSurroundingWhitespace)
        );
        assert_eq!(
            AssetName::new("Machine\nA"),
            Err(AssetError::NameContainsControlCharacter)
        );
    }

    #[test]
    fn name_limit_counts_unicode_scalar_values() {
        assert!(AssetName::new("é".repeat(MAX_ASSET_NAME_LENGTH)).is_ok());
        assert_eq!(
            AssetName::new("é".repeat(MAX_ASSET_NAME_LENGTH + 1)),
            Err(AssetError::NameTooLong {
                maximum: MAX_ASSET_NAME_LENGTH,
                actual: MAX_ASSET_NAME_LENGTH + 1,
            })
        );
    }

    #[test]
    fn adds_typed_external_identifiers() {
        let mut asset = asset();
        let identifier = ExternalIdentifier::new(
            ExternalIdentifierKind::new("manufacturer.serial-number")
                .expect("valid identifier kind"),
            Some(
                ExternalIdentifierAuthority::new("manufacturer-a")
                    .expect("valid identifier authority"),
            ),
            ExternalIdentifierValue::new("SN-100").expect("valid identifier value"),
        );

        asset
            .add_external_identifier(identifier)
            .expect("identifier should be unique on the asset");

        let stored = &asset.external_identifiers()[0];
        assert_eq!(stored.kind().as_str(), "manufacturer.serial-number");
        assert_eq!(
            stored.authority().map(AsRef::as_ref),
            Some("manufacturer-a")
        );
        assert_eq!(stored.value().as_str(), "SN-100");
    }

    #[test]
    fn rejects_an_exact_duplicate_external_identifier() {
        let mut asset = asset();
        let identifier = external_identifier("telecom.imei", "350000000000001");

        asset
            .add_external_identifier(identifier.clone())
            .expect("first identifier should be accepted");

        assert_eq!(
            asset.add_external_identifier(identifier),
            Err(AssetError::DuplicateExternalIdentifier)
        );
    }

    #[test]
    fn allows_equal_values_from_different_identifier_schemes() {
        let mut asset = asset();

        asset
            .add_external_identifier(external_identifier("operator.number", "100"))
            .expect("operator number should be accepted");
        asset
            .add_external_identifier(external_identifier("manufacturer.serial-number", "100"))
            .expect("manufacturer serial should be a distinct identifier");

        assert_eq!(asset.external_identifiers().len(), 2);
    }
}

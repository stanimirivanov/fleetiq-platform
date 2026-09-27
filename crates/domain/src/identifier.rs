use std::{error::Error, fmt, str::FromStr};

const MAX_IDENTIFIER_LENGTH: usize = 128;

/// Identifies an authenticated human or workload inside `FleetIQ`.
///
/// The identifier is internal and stable across identity-provider changes.
/// Authentication adapters are responsible for mapping provider identities to
/// this value; possession of an actor identifier does not itself grant access
/// to any tenant.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ActorId(String);

/// Identifies one idempotent actor provisioning command.
///
/// The identity is platform-scoped. Exact retries resolve to the original
/// durable operation, while reuse for another actor or external identity is a
/// collision.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ActorProvisioningId(String);

/// Identifies one trusted external authentication authority configuration.
///
/// This is a stable `FleetIQ` configuration identity, such as `workforce-oidc`,
/// rather than a display name or an unvalidated token issuer supplied by a
/// caller. The authentication adapter determines which authority successfully
/// authenticated the external subject.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct IdentityAuthorityId(String);

/// Identifies one idempotent tenant access administration command.
///
/// The identifier is scoped by tenant in persistence. Retrying the same command
/// with the same identifier and content resolves to its existing audit record;
/// reusing it for different content is an identity collision.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TenantAccessChangeId(String);

/// Identifies one idempotent tenant provisioning command.
///
/// The identity is platform-scoped. An exact retry returns the original
/// durable outcome, while reuse for another tenant or administrator is a
/// collision.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TenantProvisioningId(String);

/// Identifies the tenant that owns domain data.
///
/// Tenant identity is explicit so application and persistence boundaries cannot
/// silently fall back to a shared or default tenant.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TenantId(String);

/// Identifies a physical or logical asset inside a tenant.
///
/// The identifier deliberately carries no assumption about asset kind. A vehicle,
/// machine, subsystem, sensor, and communication device all use the same identity
/// mechanism and receive their meaning from the semantic asset model.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AssetId(String);

/// Identifies a configurable asset type.
///
/// Asset types give assets their semantic meaning without compiling
/// industry-specific categories into the domain crate.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AssetTypeId(String);

/// Identifies a relationship record across its recorded revisions.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AssetRelationshipId(String);

/// Identifies a physical or logical source exposed by one communication device.
///
/// Endpoint identity is scoped by the owning device asset. Two devices may use
/// the same endpoint identifier without referring to the same source.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DeviceEndpointId(String);

/// Identifies the configurable category of a communication endpoint.
///
/// Examples can include an onboard source, a CAN interface, a serial channel,
/// or an identified peripheral. The vocabulary remains data-driven so the core
/// domain does not encode one vendor's hardware layout.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DeviceEndpointKindId(String);

/// Identifies a signal emitted by a protocol or device profile at one endpoint.
///
/// Examples can include a normalized vendor parameter name or a stable field
/// identity from a custom protocol package. Its meaning remains scoped by the
/// source device endpoint and the decoder/profile contract that produced it.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceSignalId(String);

/// Identifies one signal-to-property attribution across recorded revisions.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SignalBindingId(String);

/// Identifies the stable semantic meaning of a directed asset relationship.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RelationshipTypeId(String);

/// Identifies the stable semantic meaning of an asset property.
///
/// A property definition can be shared by multiple asset types so calculators
/// can depend on meaning rather than a device vendor's field name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PropertyDefinitionId(String);

/// Identifies the canonical unit used by a numeric property definition.
///
/// Unit identifiers are data-driven. A later configuration layer may use a
/// standard vocabulary such as UCUM without coupling this crate to one catalogue.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UnitId(String);

/// Identifies the meaning of an identifier assigned outside `FleetIQ`.
///
/// Examples include `telecom.imei`, `manufacturer.serial-number`, and an
/// operator-specific registration scheme.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExternalIdentifierKind(String);

/// Identifies the organization or system that issued an external identifier.
///
/// An authority is optional because some identifier kinds already define a
/// globally sufficient scope. It is useful for manufacturer and operator
/// numbering schemes that may otherwise overlap.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExternalIdentifierAuthority(String);

/// Stores the value assigned to an asset by an external identification scheme.
///
/// `FleetIQ` preserves the value exactly after validation. Interpretation and
/// canonicalization belong to the definition of its identifier kind.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExternalIdentifierValue(String);

/// Describes why a domain identifier could not be constructed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdentifierError {
    /// The supplied value is empty or contains only whitespace.
    Empty,
    /// The supplied value begins or ends with whitespace.
    SurroundingWhitespace,
    /// The supplied value exceeds the domain-wide size limit.
    TooLong {
        /// The maximum accepted Unicode scalar-value count.
        maximum: usize,
        /// The Unicode scalar-value count in the rejected value.
        actual: usize,
    },
    /// The supplied value contains a control character.
    ControlCharacter,
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("identifier must not be empty"),
            Self::SurroundingWhitespace => {
                formatter.write_str("identifier must not have surrounding whitespace")
            }
            Self::TooLong { maximum, actual } => write!(
                formatter,
                "identifier contains {actual} characters; the maximum is {maximum}"
            ),
            Self::ControlCharacter => {
                formatter.write_str("identifier must not contain control characters")
            }
        }
    }
}

impl Error for IdentifierError {}

macro_rules! domain_identifier {
    ($type:ident) => {
        impl $type {
            /// Constructs an identifier after validating its domain invariants.
            ///
            /// # Errors
            ///
            /// Returns [`IdentifierError`] when `value` is empty, has surrounding
            /// whitespace, contains a control character, or is longer than 128
            /// Unicode scalar values.
            pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
                let value = value.into();
                validate(&value)?;
                Ok(Self(value))
            }

            /// Returns the identifier as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consumes the identifier and returns its text representation.
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl AsRef<str> for $type {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl fmt::Display for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $type {
            type Err = IdentifierError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }
    };
}

domain_identifier!(TenantId);
domain_identifier!(ActorId);
domain_identifier!(ActorProvisioningId);
domain_identifier!(IdentityAuthorityId);
domain_identifier!(TenantAccessChangeId);
domain_identifier!(TenantProvisioningId);
domain_identifier!(AssetId);
domain_identifier!(AssetRelationshipId);
domain_identifier!(DeviceEndpointId);
domain_identifier!(DeviceEndpointKindId);
domain_identifier!(SourceSignalId);
domain_identifier!(SignalBindingId);
domain_identifier!(AssetTypeId);
domain_identifier!(RelationshipTypeId);
domain_identifier!(PropertyDefinitionId);
domain_identifier!(UnitId);
domain_identifier!(ExternalIdentifierKind);
domain_identifier!(ExternalIdentifierAuthority);
domain_identifier!(ExternalIdentifierValue);

fn validate(value: &str) -> Result<(), IdentifierError> {
    if value.trim().is_empty() {
        return Err(IdentifierError::Empty);
    }
    if value.trim() != value {
        return Err(IdentifierError::SurroundingWhitespace);
    }
    if value.chars().any(char::is_control) {
        return Err(IdentifierError::ControlCharacter);
    }

    let actual = value.chars().count();
    if actual > MAX_IDENTIFIER_LENGTH {
        return Err(IdentifierError::TooLong {
            maximum: MAX_IDENTIFIER_LENGTH,
            actual,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_preserve_valid_external_text() {
        let asset = AssetId::new("asset:machine/primary").expect("identifier should be valid");

        assert_eq!(asset.as_str(), "asset:machine/primary");
        assert_eq!(asset.to_string(), "asset:machine/primary");
    }

    #[test]
    fn identifier_types_can_hold_the_same_text_without_becoming_interchangeable() {
        let tenant = TenantId::new("operator-a").expect("identifier should be valid");
        let asset = AssetId::new("operator-a").expect("identifier should be valid");

        assert_eq!(tenant.as_str(), asset.as_str());
    }

    #[test]
    fn rejects_empty_and_whitespace_only_values() {
        assert_eq!(TenantId::new(""), Err(IdentifierError::Empty));
        assert_eq!(TenantId::new("   "), Err(IdentifierError::Empty));
    }

    #[test]
    fn rejects_surrounding_whitespace_without_silently_normalizing() {
        assert_eq!(
            AssetId::new(" asset-1"),
            Err(IdentifierError::SurroundingWhitespace)
        );
        assert_eq!(
            AssetId::new("asset-1 "),
            Err(IdentifierError::SurroundingWhitespace)
        );
    }

    #[test]
    fn rejects_control_characters() {
        assert_eq!(
            AssetId::new("asset\n1"),
            Err(IdentifierError::ControlCharacter)
        );
    }

    #[test]
    fn length_limit_counts_unicode_scalar_values() {
        let accepted = "é".repeat(MAX_IDENTIFIER_LENGTH);
        let rejected = "é".repeat(MAX_IDENTIFIER_LENGTH + 1);

        assert!(AssetId::new(accepted).is_ok());
        assert_eq!(
            AssetId::new(rejected),
            Err(IdentifierError::TooLong {
                maximum: MAX_IDENTIFIER_LENGTH,
                actual: MAX_IDENTIFIER_LENGTH + 1,
            })
        );
    }
}

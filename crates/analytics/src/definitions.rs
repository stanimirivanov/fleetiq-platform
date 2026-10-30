//! Immutable declarations for reviewed first-party calculator implementations.
use crate::CalculatorRef;
use fleetiq_domain::{PropertyDefinitionRef, TenantId};
use std::{error::Error, fmt};

/// Reviewed implementation ABI; registration does not activate executable code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculatorImplementation {
    /// One numeric input, decimal mean output in the same canonical unit.
    /// Configuration uses PR1's window/input limits; minimum samples is 1..=1024.
    WindowMeanV1,
}
impl CalculatorImplementation {
    /// Resolves only a reviewed first-party implementation identifier.
    /// # Errors
    /// Rejects uploaded code, unknown names and unreviewed ABI versions.
    pub fn parse(value: &str) -> Result<Self, UnknownCalculatorImplementation> {
        match value {
            "window-mean-v1" => Ok(Self::WindowMeanV1),
            _ => Err(UnknownCalculatorImplementation),
        }
    }
    /// Returns the immutable implementation/configuration contract identifier.
    pub const fn as_str(self) -> &'static str {
        "window-mean-v1"
    }
}
/// An implementation is not admitted by this binary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnknownCalculatorImplementation;
impl fmt::Display for UnknownCalculatorImplementation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown calculator implementation")
    }
}
impl Error for UnknownCalculatorImplementation {}

/// Tenant-owned release with exact input/output property versions.
/// The store verifies reference ownership, numeric kinds and matching units.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalculatorDefinition {
    /// Tenant whose property catalogue owns both references.
    pub tenant: TenantId,
    /// Immutable release identity, independent of the reviewed implementation ABI.
    pub release: CalculatorRef,
    /// Reviewed implementation and supported configuration contract.
    pub implementation: CalculatorImplementation,
    /// Exact semantic input definition; role is `input`.
    pub input: PropertyDefinitionRef,
    /// Exact semantic output definition; role is `mean`.
    pub output: PropertyDefinitionRef,
}
/// Result of registering immutable definition content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculatorRegistrationOutcome {
    /// This call inserted the release.
    Registered,
    /// The complete identical declaration was already durable.
    AlreadyPresent,
}

/// Consumer-owned immutable registration boundary; never activates a release.
pub trait CalculatorDefinitionRegistry: Send + Sync {
    /// Storage, identity-collision or invalid-reference error.
    type Error: Error + Send + Sync + 'static;
    /// Registers all declared content atomically or compares an exact retry.
    /// # Errors
    /// Rejects changed content, invalid tenant-owned references and storage failures.
    fn register<'a>(
        &'a self,
        definition: &'a CalculatorDefinition,
    ) -> impl std::future::Future<Output = Result<CalculatorRegistrationOutcome, Self::Error>> + Send + 'a;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_reviewed_abi_is_admitted() {
        assert_eq!(
            CalculatorImplementation::parse("window-mean-v1")
                .unwrap()
                .as_str(),
            "window-mean-v1"
        );
        for invalid in [
            "",
            "window-mean-v2",
            " window-mean-v1",
            "file:///plugin.wasm",
        ] {
            assert_eq!(
                CalculatorImplementation::parse(invalid),
                Err(UnknownCalculatorImplementation)
            );
        }
    }
}

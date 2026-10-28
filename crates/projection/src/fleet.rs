//! Read-only fleet inventory, bounded position evidence and explicit mapping.
mod location;
use fleetiq_access::{AuthorizedTenant, ReadMetadata, ReadTelemetry};
use fleetiq_domain::{AssetId, AssetName, AssetTypeRef, PropertyDefinitionRef, TenantId};
use fleetiq_history::StoredPropertyChange;
pub use location::{LocationState, ObservedLocation, classify_location};
use std::{error::Error, fmt, future::Future};
/// Explicit encoding for both coordinates; property names imply no scaling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinateEncoding {
    /// Numeric degrees with canonical/source unit `deg`.
    Degrees,
    /// Signed integer degrees times ten million, with no source/canonical unit.
    E7,
}
/// Caller-selected exact immutable definitions, not a persisted configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocationMapping {
    latitude: PropertyDefinitionRef,
    longitude: PropertyDefinitionRef,
    encoding: CoordinateEncoding,
}
impl LocationMapping {
    /// Requires distinct property identities; swapped roles are explicit input.
    /// # Errors
    /// Returns [`InvalidFleetQuery`] when both axes name the same property.
    pub fn new(
        latitude: PropertyDefinitionRef,
        longitude: PropertyDefinitionRef,
        encoding: CoordinateEncoding,
    ) -> Result<Self, InvalidFleetQuery> {
        if latitude.id() == longitude.id() {
            return Err(InvalidFleetQuery);
        }
        Ok(Self {
            latitude,
            longitude,
            encoding,
        })
    }
    /// Exact latitude definition.
    pub const fn latitude(&self) -> &PropertyDefinitionRef {
        &self.latitude
    }
    /// Exact longitude definition.
    pub const fn longitude(&self) -> &PropertyDefinitionRef {
        &self.longitude
    }
    /// Declared representation shared by both axes.
    pub const fn encoding(&self) -> CoordinateEncoding {
        self.encoding
    }
}
/// One exclusive keyset page; counts always cover the entire authorized tenant.
#[derive(Clone, Debug)]
pub struct FleetQuery {
    mapping: LocationMapping,
    limit: u16,
    after: Option<AssetId>,
}
impl FleetQuery {
    /// Bounds output to 1–100 assets and at most 200 immutable source facts.
    /// # Errors
    /// Returns [`InvalidFleetQuery`] for an out-of-range page size.
    pub fn new(
        mapping: LocationMapping,
        limit: u16,
        after: Option<AssetId>,
    ) -> Result<Self, InvalidFleetQuery> {
        if !(1..=100).contains(&limit) {
            return Err(InvalidFleetQuery);
        }
        Ok(Self {
            mapping,
            limit,
            after,
        })
    }
    /// Explicit coordinate mapping.
    pub const fn mapping(&self) -> &LocationMapping {
        &self.mapping
    }
    /// Maximum returned rows.
    pub const fn limit(&self) -> u16 {
        self.limit
    }
    /// Exclusive byte-ordered asset key, scoped by the retained query parameters.
    pub const fn after(&self) -> Option<&AssetId> {
        self.after.as_ref()
    }
}
/// Invalid axis identity or page bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidFleetQuery;
impl fmt::Display for InvalidFleetQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid fleet query")
    }
}
impl Error for InvalidFleetQuery {}
/// Safe inventory labels and bounded original coordinate candidates.
#[derive(Clone, Debug)]
pub struct FleetAsset {
    /// Stable tenant-owned asset key.
    pub id: AssetId,
    /// Operator label, independent of identity.
    pub name: AssetName,
    /// Installed immutable asset type.
    pub asset_type: AssetTypeRef,
    /// Both exact coordinate definitions are declared by this installed type.
    pub mapped: bool,
    /// A configured source signal repeats at the same device time in its packet.
    pub ambiguous: bool,
    /// Current latitude candidate; no last-good fallback is selected.
    pub latitude: Option<StoredPropertyChange>,
    /// Current longitude candidate.
    pub longitude: Option<StoredPropertyChange>,
}
/// Server-owned inventory/projection coverage, never health or connectivity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FleetSummary {
    /// All assets in this tenant, independent of the page cursor.
    pub assets: u64,
    /// Registered communication-device capabilities, which are also assets.
    pub communication_devices: u64,
    /// Assets declaring both selected exact property versions.
    pub mapped_assets: u64,
    /// Assets with at least one materialized property of any kind.
    pub assets_with_current_properties: u64,
}
/// Metadata, counters, lag and coordinate candidates share one consistent view.
#[derive(Clone, Debug)]
pub struct FleetSnapshot {
    /// Rebuild generation, zero before projection initialization.
    pub generation: u64,
    /// Committed tenant revision.
    pub revision: u64,
    /// Pending committed interpretation work, not transport liveness.
    pub pending: bool,
    /// Both immutable definitions have supported kinds and units for the encoding.
    pub mapping_supported: bool,
    /// Complete tenant inventory/coverage counts.
    pub summary: FleetSummary,
    /// Byte-ordered bounded asset page, including assets without usable positions.
    pub assets: Vec<FleetAsset>,
    /// Exclusive continuation key; absence means this snapshot had no further row.
    pub next_after: Option<AssetId>,
}
/// Consistent fleet read port, consumed by this capability.
pub trait FleetReadRepository: Send + Sync {
    /// Storage or invalid durable-state error.
    type Error: Error + Send + Sync + 'static;
    /// Returns None only when either selected definition does not exist.
    fn fleet(
        &self,
        tenant: &TenantId,
        query: &FleetQuery,
    ) -> impl Future<Output = Result<Option<FleetSnapshot>, Self::Error>> + Send;
}
/// Independent inventory and telemetry capabilities must belong to one actor/tenant.
pub struct FleetReadService<R> {
    repository: R,
}
impl<R> FleetReadService<R> {
    /// Composes a consistent read adapter without starting background work.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}
impl<R: FleetReadRepository> FleetReadService<R> {
    /// Reads only after matching both explicit capability proofs.
    /// # Errors
    /// Returns scope mismatch before touching storage, or the repository failure.
    pub async fn read(
        &self,
        metadata: AuthorizedTenant<ReadMetadata>,
        telemetry: AuthorizedTenant<ReadTelemetry>,
        query: &FleetQuery,
    ) -> Result<Option<FleetSnapshot>, FleetReadError<R::Error>> {
        if metadata.actor_id() != telemetry.actor_id()
            || metadata.tenant_id() != telemetry.tenant_id()
        {
            return Err(FleetReadError::ScopeMismatch);
        }
        self.repository
            .fleet(telemetry.tenant_id(), query)
            .await
            .map_err(FleetReadError::Repository)
    }
}
/// Authorization proof mismatch or safe-to-classify adapter failure.
#[derive(Debug)]
pub enum FleetReadError<E> {
    /// No storage access is allowed using mixed capability proofs.
    ScopeMismatch,
    /// Consistent storage read failed; never fabricate empty success.
    Repository(E),
}
impl<E: fmt::Display> fmt::Display for FleetReadError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScopeMismatch => f.write_str("fleet authority scope mismatch"),
            Self::Repository(e) => write!(f, "fleet read failed: {e}"),
        }
    }
}
impl<E: Error + 'static> Error for FleetReadError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(e) => Some(e),
            Self::ScopeMismatch => None,
        }
    }
}

//! Revisioned host routing; identifier evidence never authenticates a peer.
mod service;
#[cfg(test)]
mod tests;
use crate::{ProtocolAdmissionWorld, ProtocolPackageRelease};
use fleetiq_domain::{ActorId, AssetId, ExternalIdentifierAuthority, TenantId, UnixTimeMillis};
use fleetiq_protocol_sdk::DeviceIdentityClaim;
use fleetiq_telemetry::ProtocolPackageRef;
pub use service::{PinnedProtocolSession, ProtocolRoutingService};
use std::{error::Error, fmt, future::Future, num::NonZeroU32};

/// Host-configured channel identity, independent of a package-supplied claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolChannelId(String);
impl ProtocolChannelId {
    /// Validates 1..=128 ASCII letters, digits, dots, underscores or hyphens.
    /// # Errors
    /// Rejects malformed channel names without retaining them in diagnostics.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidProtocolChannel> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        {
            return Err(InvalidProtocolChannel);
        }
        Ok(Self(value))
    }
    /// Returns the exact host configuration name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// Invalid bounded channel identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidProtocolChannel;
impl fmt::Display for InvalidProtocolChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid protocol channel")
    }
}
impl Error for InvalidProtocolChannel {}

/// Exact immutable package/world selected by an operator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolRouteSelection {
    /// Registered immutable release; never a floating version.
    pub package: ProtocolPackageRef,
    /// Supported world for which current admission evidence must exist.
    pub world: ProtocolAdmissionWorld,
}
/// One durable route generation, including retirement and original attribution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolDeviceRoute {
    /// Owning tenant.
    pub tenant_id: TenantId,
    /// Registered communication device's asset identity.
    pub device_asset_id: AssetId,
    /// Host channel namespace.
    pub channel_id: ProtocolChannelId,
    /// Monotonic generation; any change invalidates previously pinned sessions.
    pub revision: NonZeroU32,
    /// None retires this device/channel route, without erasing history.
    pub selection: Option<ProtocolRouteSelection>,
    /// Exact admission policy pinned by this generation; None on retirement.
    pub policy_version: Option<i32>,
    /// First operator for this exact command outcome.
    pub actor_id: ActorId,
    /// Original host command-receipt time in Unix milliseconds.
    pub recorded_at: UnixTimeMillis,
}
/// Host-attributed replacement request. Zero expects no previous generation.
#[derive(Clone, Debug)]
pub struct ProtocolRouteCommand {
    /// Selected tenant, supplied by authorization evidence.
    pub tenant_id: TenantId,
    /// Registered communication device.
    pub device_asset_id: AssetId,
    /// Host channel identity.
    pub channel_id: ProtocolChannelId,
    /// Current generation the operator observed; prevents lost updates.
    pub expected_revision: u32,
    /// Exact admitted release/world, or explicit retirement.
    pub selection: Option<ProtocolRouteSelection>,
    /// Authenticated command author.
    pub actor_id: ActorId,
    /// First-receipt time; excluded from retry matching.
    pub recorded_at: UnixTimeMillis,
}
/// Explicit outcomes distinguish conflicting configuration from infrastructure failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolRouteWrite {
    /// Durable change or exact same-actor retry, preserving the first receipt.
    Committed(ProtocolDeviceRoute),
    /// Another generation/content won or the revision range is exhausted.
    Conflict,
    /// Tenant/device/content/current-policy admission prerequisite is absent.
    Unavailable,
}
/// Explicit identifier-only policy selected by a trusted listener configuration.
///
/// This is routing policy, not cryptographic device authentication. Never build
/// it from guest claims or a browser request. The future transport owns peer
/// authentication, listener enablement and the channel configuration lifecycle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredIdentityChannel {
    /// Host-selected tenant: identifiers are never searched across tenants.
    pub tenant_id: TenantId,
    /// Host-selected channel with explicit device assignments.
    pub channel_id: ProtocolChannelId,
    /// Host-selected registered identifier issuer; None means the implicit issuer.
    pub authority: Option<ExternalIdentifierAuthority>,
    /// Host-selected exact decoder release.
    pub selection: ProtocolRouteSelection,
}
/// Snapshot resolved from active configuration and a registered external identifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolSessionRoute {
    /// Exact current route generation, never an authenticated peer credential.
    pub route: ProtocolDeviceRoute,
    /// Exact catalogue content identity; compilation must use these bytes.
    pub release: ProtocolPackageRelease,
}
/// Consumed routing port. Writes serialize revisions and preserve immutable history.
pub trait ProtocolRoutingRepository: Send + Sync {
    /// Storage or persisted-data reconstruction failure.
    type Error: Error + Send + Sync + 'static;
    /// Reads the latest generation, including a retired route.
    /// # Errors
    /// Reports storage or reconstruction failure.
    fn find(
        &self,
        tenant: &TenantId,
        device: &AssetId,
        channel: &ProtocolChannelId,
    ) -> impl Future<Output = Result<Option<ProtocolDeviceRoute>, Self::Error>> + Send;
    /// Applies a compare-and-swap replacement or resolves an exact same-actor retry.
    /// # Errors
    /// Reports infrastructure failure; expected denials use [`ProtocolRouteWrite`].
    fn change(
        &self,
        command: &ProtocolRouteCommand,
    ) -> impl Future<Output = Result<ProtocolRouteWrite, Self::Error>> + Send;
    /// Resolves one current, active, stored/admitted route within the trusted scope.
    /// # Errors
    /// Reports storage/reconstruction failure. Ineligible identities return None.
    fn resolve(
        &self,
        channel: &RegisteredIdentityChannel,
        claim: &DeviceIdentityClaim,
    ) -> impl Future<Output = Result<Option<ProtocolSessionRoute>, Self::Error>> + Send;
}

use super::{
    ProtocolChannelId, ProtocolDeviceRoute, ProtocolRouteCommand, ProtocolRouteSelection,
    ProtocolRouteWrite, ProtocolRoutingRepository, ProtocolSessionRoute, RegisteredIdentityChannel,
};
use fleetiq_access::{AuthorizedTenant, ManageProtocols};
use fleetiq_domain::{AssetId, UnixTimeMillis};
use fleetiq_protocol_sdk::DeviceIdentityClaim;

/// Pinned handoff retaining the trusted scope and redacted untrusted claim.
/// Revalidate before every transport turn; possession alone grants no execution
/// or peer authentication authority. A validation is a point-in-time snapshot:
/// already-started bounded work may complete after concurrent retirement.
#[derive(Clone, Debug)]
pub struct PinnedProtocolSession {
    channel: RegisteredIdentityChannel,
    claim: DeviceIdentityClaim,
    snapshot: ProtocolSessionRoute,
}
impl PinnedProtocolSession {
    /// Returns the exact device, generation and content selected by the host.
    pub const fn snapshot(&self) -> &ProtocolSessionRoute {
        &self.snapshot
    }
}
/// Coordinates protected commissioning and internal host routing without transport I/O.
pub struct ProtocolRoutingService<R> {
    repository: R,
}
impl<R: ProtocolRoutingRepository> ProtocolRoutingService<R> {
    /// Composes the consuming port; no compiler, socket or device credentials are owned.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
    /// Resolves only explicit host scope to a currently admitted identity package.
    /// # Errors
    /// Reports storage failures; absence never permits guest execution.
    pub async fn bootstrap(
        &self,
        channel: &RegisteredIdentityChannel,
    ) -> Result<Option<crate::ProtocolPackageRelease>, R::Error>
    where
        R: crate::LiveRoutingRepository,
    {
        if channel.selection.world != crate::ProtocolAdmissionWorld::Identity {
            return Ok(None);
        }
        Ok(self.repository.bootstrap(channel).await?.filter(|release| {
            release.tenant_id() == &channel.tenant_id
                && release.package() == &channel.selection.package
        }))
    }
    /// Reads an authorized tenant's exact device/channel configuration.
    /// # Errors
    /// Reports storage or reconstruction failure.
    pub async fn find(
        &self,
        authorization: AuthorizedTenant<ManageProtocols>,
        device: &AssetId,
        channel: &ProtocolChannelId,
    ) -> Result<Option<ProtocolDeviceRoute>, R::Error> {
        self.repository
            .find(authorization.tenant_id(), device, channel)
            .await
    }
    /// Replaces or retires one route with trusted actor/tenant/time attribution.
    /// # Errors
    /// Reports storage failure. Revision and prerequisite denials remain explicit.
    pub async fn change(
        &self,
        authorization: AuthorizedTenant<ManageProtocols>,
        device: AssetId,
        channel: ProtocolChannelId,
        expected_revision: u32,
        selection: Option<ProtocolRouteSelection>,
        now: UnixTimeMillis,
    ) -> Result<ProtocolRouteWrite, R::Error> {
        self.repository
            .change(&ProtocolRouteCommand {
                tenant_id: authorization.tenant_id().clone(),
                device_asset_id: device,
                channel_id: channel,
                expected_revision,
                selection,
                actor_id: authorization.actor_id().clone(),
                recorded_at: now,
            })
            .await
    }
    /// Resolves a registered claim only under explicit host identifier-routing policy.
    /// This does not authenticate the network peer and does not invoke a guest.
    /// # Errors
    /// Reports repository failure; missing/inactive/mismatched routes yield None.
    pub async fn resolve(
        &self,
        channel: RegisteredIdentityChannel,
        claim: DeviceIdentityClaim,
    ) -> Result<Option<PinnedProtocolSession>, R::Error> {
        Ok(self
            .repository
            .resolve(&channel, &claim)
            .await?
            .filter(|snapshot| {
                snapshot.route.tenant_id == channel.tenant_id
                    && snapshot.route.channel_id == channel.channel_id
                    && snapshot.route.selection.as_ref() == Some(&channel.selection)
                    && snapshot.route.policy_version == Some(crate::PROTOCOL_ADMISSION_POLICY)
                    && snapshot.release.tenant_id() == &channel.tenant_id
                    && snapshot.release.package() == &channel.selection.package
            })
            .map(|snapshot| PinnedProtocolSession {
                channel,
                claim,
                snapshot,
            }))
    }
    /// Rechecks current tenant/configuration/content/identifier eligibility.
    /// Call immediately before each future bounded transport turn. Reconfiguration
    /// invalidates an old handoff even if it later selects identical bytes again.
    /// # Errors
    /// Reports repository failure; no failure can be treated as a valid session.
    pub async fn revalidate(&self, session: &PinnedProtocolSession) -> Result<bool, R::Error> {
        Ok(self
            .repository
            .resolve(&session.channel, &session.claim)
            .await?
            .as_ref()
            == Some(&session.snapshot))
    }
}

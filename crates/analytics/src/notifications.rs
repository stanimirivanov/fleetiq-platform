//! Native notification snapshots shared by durable work and the host transport.
use crate::{AlertPhase, AlertRuleDefinition, AlertRuleId};
use fleetiq_domain::{TenantId, UnixTimeMillis};
/// One database-clock, generation-fenced incident delivery attempt.
#[derive(Clone, Debug)]
pub struct AlertNotificationLease {
    /// Configured endpoint digest, never a tenant-supplied URL.
    pub destination: [u8; 32],
    /// Owning tenant, retained in remote identity.
    pub tenant: TenantId,
    /// Exact rule series.
    pub rule: AlertRuleId,
    /// Immutable rule revision.
    pub revision: u32,
    /// Original firing version; stable incident identity through resolution.
    pub firing_version: i64,
    /// Latest terminal transition represented by this snapshot.
    pub version: i64,
    /// Monotonic durable lease generation.
    pub generation: i64,
    /// Firing or resolved; other phases cannot be delivered.
    pub phase: AlertPhase,
    /// Original incident event-time start.
    pub starts_at: UnixTimeMillis,
    /// Resolution event time or finite database-clock firing expiry.
    pub ends_at: UnixTimeMillis,
    /// Immutable selector; no raw input or prepared payload expansion.
    pub definition: AlertRuleDefinition,
}
/// Retained bounded transport category, excluding URLs, bodies and credentials.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertDeliveryFailure {
    /// Connection, timeout or ambiguous completion.
    Transport,
    /// Non-success HTTP status, including redirects.
    Rejected,
    /// Remote body exceeded the read budget.
    ResponseLimit,
    /// A snapshot cannot be represented safely on the wire.
    InvalidPayload,
}
impl AlertDeliveryFailure {
    /// Stable storage category for operations; never an upstream exception.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Transport => "transport",
            Self::Rejected => "rejected",
            Self::ResponseLimit => "response_limit",
            Self::InvalidPayload => "invalid_payload",
        }
    }
}

//! Explicit bounded durable exports; selections never change during retry or restart.
use crate::{PropertyHistoryQuery, StoredPropertyChange, restore::RestoreTicket};
use fleetiq_domain::{SignalTarget, TenantId, UnixTimeMillis};
use std::future::Future;
/// Conservative slot charge covers a bounded source file and output/work bytes.
pub const EXPORT_BYTES: i64 = 4_194_304;
/// Redacted request, coverage, quota and infrastructure failures.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ExportFailure {
    /// Invalid key, range or ticket.
    Bounds,
    /// Row, byte or exact codec profile exceeded; no truncation occurred.
    Limit,
    /// Unavailable selected coverage or conflicting original evidence.
    Coverage,
    /// Policy disabled or tenant inactive.
    Disabled,
    /// Current tenant quota cannot admit the selection or worker.
    Denied,
    /// An idempotency key was already used with another scope.
    Conflict,
    /// Persistence, codec or bounded-turn failure.
    Storage,
}
impl std::fmt::Display for ExportFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "export rejected: {self:?}")
    }
}
impl std::error::Error for ExportFailure {}
/// Tenant-scoped explicit request identity, not an authority or a file path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportKey(String);
impl ExportKey {
    /// Validates a bounded ASCII request key.
    /// # Errors
    /// Rejects empty, oversized or noncanonical keys.
    pub fn new(value: impl Into<String>) -> Result<Self, ExportFailure> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
            || !value.as_bytes()[0].is_ascii_alphanumeric()
        {
            return Err(ExportFailure::Bounds);
        }
        Ok(Self(value))
    }
    /// Original validated request key.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// Exact bounded evidence scope; no continuation or implicit archive discovery.
#[derive(Clone)]
pub struct ExportRequest {
    tenant: TenantId,
    query: PropertyHistoryQuery,
    restore: Option<RestoreTicket>,
}
impl ExportRequest {
    /// Validates a nonempty half-open window of at most seven days.
    /// # Errors
    /// Rejects invalid or excessive windows before reading any evidence.
    pub fn new(
        tenant: TenantId,
        target: SignalTarget,
        start: i64,
        end: i64,
        restore: Option<RestoreTicket>,
    ) -> Result<Self, ExportFailure> {
        Ok(Self {
            tenant,
            query: PropertyHistoryQuery::new(
                target,
                UnixTimeMillis::new(start),
                UnixTimeMillis::new(end),
                16,
                None,
            )
            .map_err(|_| ExportFailure::Bounds)?,
            restore,
        })
    }
    /// Tenant owning the request and every selected interpretation.
    pub const fn tenant(&self) -> &TenantId {
        &self.tenant
    }
    /// Exact immutable target and window.
    pub const fn query(&self) -> &PropertyHistoryQuery {
        &self.query
    }
    /// Explicit selected restored generation, if any.
    pub const fn restore(&self) -> Option<RestoreTicket> {
        self.restore
    }
}
/// Stable job identity; selection generation one never changes with attempt retries.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ExportTicket(i64);
impl ExportTicket {
    /// Reconstructs a positive tenant-scoped job identity; grants no authority.
    /// # Errors
    /// Rejects nonpositive identities.
    pub fn new(id: i64) -> Result<Self, ExportFailure> {
        if id <= 0 {
            Err(ExportFailure::Bounds)
        } else {
            Ok(Self(id))
        }
    }
    /// Database-owned job identity.
    pub const fn id(self) -> i64 {
        self.0
    }
}
/// Exact temporary artifact and its original selected scope/lifetime.
pub struct ExportArtifact {
    bytes: Vec<u8>,
    expires_at_ms: i64,
    restore: Option<RestoreTicket>,
}
impl ExportArtifact {
    /// Reconstructs verified bounded bytes supplied by the trusted persistence adapter.
    /// # Errors
    /// Rejects empty or oversized files; construction grants no authority.
    pub fn new(
        bytes: Vec<u8>,
        expires_at_ms: i64,
        restore: Option<RestoreTicket>,
    ) -> Result<Self, ExportFailure> {
        if bytes.is_empty() || bytes.len() > 2_097_152 {
            return Err(ExportFailure::Limit);
        }
        Ok(Self {
            bytes,
            expires_at_ms,
            restore,
        })
    }
    /// Complete exact property-history-v1 Parquet file.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Original signed Unix-millisecond expiry; retries never extend it.
    pub const fn expires_at_ms(&self) -> i64 {
        self.expires_at_ms
    }
    /// Existing exact codec version, without a new export format.
    pub const fn format_version(&self) -> u8 {
        1
    }
    /// Frozen selected generation, distinct from attempt retries.
    pub const fn selection_generation(&self) -> u8 {
        1
    }
    /// Explicit restored scope; absence means hot snapshot, never complete interval coverage.
    pub const fn restore(&self) -> Option<RestoreTicket> {
        self.restore
    }
}
/// Availability never implies complete interval coverage.
#[derive(Debug, Eq, PartialEq)]
pub enum ExportStatus {
    /// No job owned by this tenant.
    Missing,
    /// Frozen selection awaits verified publication.
    Pending,
    /// Exact artifact available until recorded expiry.
    Ready,
    /// Explicit terminal cancellation.
    Cancelled,
    /// Artifact unavailable even if physical cleanup is pending.
    Expired,
}
/// Frozen bounded source and independently fenced worker ownership.
pub struct ExportLease {
    /// Owning tenant.
    pub tenant: TenantId,
    /// Stable selected generation, independent of this attempt.
    pub ticket: ExportTicket,
    /// Monotone retry fence.
    pub attempt: i64,
    /// Frozen exact-format selection, at most 2 MiB.
    pub payload: Vec<u8>,
}
/// Explicit worker admission outcome.
pub enum ExportClaim {
    /// No claimable work.
    Idle,
    /// Tenant policy currently cannot admit processing.
    Denied,
    /// One thirty-second attempt owns the bounded work.
    Claimed(ExportLease),
}
/// One bounded worker result.
#[derive(Debug, Eq, PartialEq)]
pub enum ExportTick {
    /// No claimable work.
    Idle,
    /// Current quotas prohibit processing.
    Denied,
    /// Verified artifact published.
    Ready,
    /// Ownership, cancellation, policy or lifetime changed.
    Fenced,
}
/// Consumer-owned exact codec boundary; vendor representations stay outside history.
pub trait ExportCodec: Send + Sync {
    /// Encodes complete original evidence under the existing exact file profile.
    /// # Errors
    /// Rejects unsupported/oversized/invalid evidence, never silently truncates it.
    fn encode(
        &self,
        tenant: &TenantId,
        rows: &[StoredPropertyChange],
    ) -> Result<Vec<u8>, ExportFailure>;
    /// Decodes the complete bounded tenant-owned frozen file.
    /// # Errors
    /// Rejects malformed, oversized or cross-tenant evidence.
    fn decode(
        &self,
        tenant: &TenantId,
        bytes: &[u8],
    ) -> Result<Vec<StoredPropertyChange>, ExportFailure>;
}
/// History-owned durable worker publication boundary.
pub trait ExportRepository: Send + Sync {
    /// Claims one exact pending job under current quota and a thirty-second fence.
    fn claim(
        &self,
        tenant: &TenantId,
        ticket: ExportTicket,
    ) -> impl Future<Output = Result<ExportClaim, ExportFailure>> + Send;
    /// Publishes only matching exact bytes under the current unexpired attempt.
    fn complete(
        &self,
        lease: &ExportLease,
        bytes: &[u8],
    ) -> impl Future<Output = Result<bool, ExportFailure>> + Send;
    /// Acknowledges stopped work without reopening or changing its frozen membership.
    fn abandon(
        &self,
        lease: &ExportLease,
    ) -> impl Future<Output = Result<(), ExportFailure>> + Send;
}
/// Runs one durable attempt; the trusted host enforces a twenty-second turn deadline.
/// # Errors
/// Returns redacted quota, codec and publication failures; uncertain attempts expire.
pub async fn export_once<R: ExportRepository, C: ExportCodec>(
    repo: &R,
    codec: &C,
    tenant: &TenantId,
    ticket: ExportTicket,
) -> Result<ExportTick, ExportFailure> {
    let lease = match repo.claim(tenant, ticket).await? {
        ExportClaim::Idle => return Ok(ExportTick::Idle),
        ExportClaim::Denied => return Ok(ExportTick::Denied),
        ExportClaim::Claimed(lease) => lease,
    };
    let output = (|| {
        let rows = codec.decode(tenant, &lease.payload)?;
        if rows.is_empty() || rows.len() > 16 {
            return Err(ExportFailure::Limit);
        }
        let bytes = codec.encode(tenant, &rows)?;
        if bytes.is_empty() || bytes.len() > 2_097_152 || bytes != lease.payload {
            return Err(ExportFailure::Coverage);
        }
        Ok(bytes)
    })();
    let result = match output {
        Ok(bytes) => repo.complete(&lease, &bytes).await.map(|ready| {
            if ready {
                ExportTick::Ready
            } else {
                ExportTick::Fenced
            }
        }),
        Err(e) => Err(e),
    };
    repo.abandon(&lease).await?;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_identities_are_bounded_and_never_paths() {
        for bad in ["", "../escape", "_leading", "UPPER", "a/b"] {
            assert_eq!(ExportKey::new(bad), Err(ExportFailure::Bounds));
        }
        assert!(ExportKey::new("a".repeat(129)).is_err());
        assert!(ExportKey::new("export-one.2").is_ok());
        assert!(ExportTicket::new(0).is_err());
    }
}

//! Explicit generation-bound temporary restore; never ingestion replay or deletion authority.
use crate::{StoredPropertyChange, archive_work::ArchiveObject};
use fleetiq_domain::TenantId;
use std::future::Future;
/// Redacted restore failure; durable requests remain recoverable.
#[derive(Debug)]
pub struct RestoreFailure;
impl std::fmt::Display for RestoreFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("restore failed")
    }
}
impl std::error::Error for RestoreFailure {}
/// Exact manifest and explicit request generation, suitable for later scoped query cursors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RestoreTicket {
    manifest: i64,
    generation: i64,
}
impl RestoreTicket {
    /// Reconstructs a positive tenant-scoped request identity; not an authorization grant.
    /// # Errors
    /// Rejects nonpositive manifest/request identities.
    pub fn new(manifest: i64, generation: i64) -> Result<Self, RestoreFailure> {
        if manifest <= 0 || generation <= 0 {
            return Err(RestoreFailure);
        }
        Ok(Self {
            manifest,
            generation,
        })
    }
    /// Selected immutable archive manifest identity.
    pub const fn manifest(self) -> i64 {
        self.manifest
    }
    /// Explicit request generation; attempt retries do not change it.
    pub const fn generation(self) -> i64 {
        self.generation
    }
}
/// Availability is explicit; expired/cancelled/missing never means complete empty history.
#[derive(Debug, Eq, PartialEq)]
pub enum RestoreStatus {
    /// Request absent or replaced by a later generation.
    Missing,
    /// Awaiting verified atomic publication.
    Pending,
    /// Verified staging is available until its recorded expiry.
    Ready,
    /// Explicitly cancelled; an exact request retry never reopens it.
    Cancelled,
    /// No longer available, even if physical cleanup is pending.
    Expired,
}
/// One database-owned attempt with frozen immutable object identity.
#[derive(Clone)]
pub struct RestoreLease {
    /// Owner of the request, manifest and temporary copy.
    pub tenant: TenantId,
    /// Exact request identity.
    pub ticket: RestoreTicket,
    /// Monotone attempt fence.
    pub attempt: i64,
    /// Tenant work reservation, independent of request generation.
    pub reservation: i64,
    /// Verified manifest's complete-object descriptor.
    pub object: ArchiveObject,
}
/// Exact verified file and decoded evidence; vendor representations stay in the adapter.
pub struct RestoredSegment {
    bytes: Vec<u8>,
    changes: Vec<StoredPropertyChange>,
}
impl RestoredSegment {
    /// Accepts evidence from a trusted verifying decoder, not arbitrary caller file bytes.
    /// # Errors
    /// Rejects empty/oversized files or membership outside the sixteen-row manifest profile.
    pub fn new(bytes: Vec<u8>, changes: Vec<StoredPropertyChange>) -> Result<Self, RestoreFailure> {
        if bytes.is_empty() || bytes.len() > 2_097_152 || changes.is_empty() || changes.len() > 16 {
            return Err(RestoreFailure);
        }
        Ok(Self { bytes, changes })
    }
    /// Exact complete file, preserved without re-encoding or semantic interpretation.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Exact validated source/target/provenance evidence.
    pub fn changes(&self) -> &[StoredPropertyChange] {
        &self.changes
    }
}
/// Explicit admission and claim outcome.
pub enum RestoreClaim {
    /// No claimable work for this exact ticket.
    Idle,
    /// Lifecycle policy disabled or tenant inactive.
    Disabled,
    /// Active work/live staging exceeds current tenant allowance.
    Denied,
    /// A bounded attempt owns the work.
    Claimed(RestoreLease),
}
/// Result of one bounded restore turn.
#[derive(Debug, Eq, PartialEq)]
pub enum RestoreTick {
    /// No currently claimable work, not a completeness claim.
    Idle,
    /// Policy disabled or tenant inactive.
    Disabled,
    /// Admission denied by the tenant budget.
    Denied,
    /// Verified staging committed atomically.
    Ready,
    /// Request, attempt, policy or lease no longer permits completion.
    Fenced,
}
/// History-owned durable restore publication port.
pub trait RestoreRepository: Send + Sync {
    /// Claims one explicit request under a thirty-second lease/reservation.
    fn claim(
        &self,
        tenant: &TenantId,
        ticket: RestoreTicket,
    ) -> impl Future<Output = Result<RestoreClaim, RestoreFailure>> + Send;
    /// Publishes only exact verified bytes/membership under current ownership.
    fn complete(
        &self,
        lease: &RestoreLease,
        segment: &RestoredSegment,
    ) -> impl Future<Output = Result<bool, RestoreFailure>> + Send;
    /// Releases a failed valid attempt; cancellation/uncertainty reconcile by expiry.
    fn abandon(
        &self,
        lease: &RestoreLease,
    ) -> impl Future<Output = Result<(), RestoreFailure>> + Send;
}
/// Verifying object read/codec boundary; no re-evaluation or fallback to hot payloads.
pub trait ArchiveRestoreSource: Send + Sync {
    /// Verifies whole-file digest/length/tenant and decodes bounded original evidence.
    fn load(
        &self,
        tenant: &TenantId,
        object: ArchiveObject,
    ) -> impl Future<Output = Result<RestoredSegment, RestoreFailure>> + Send;
}
/// Runs one restore attempt; trusted callers enforce a twenty-second whole-turn deadline.
/// # Errors
/// Returns redacted admission, object, codec or publication errors.
pub async fn restore_once<R: RestoreRepository, S: ArchiveRestoreSource>(
    repo: &R,
    source: &S,
    tenant: &TenantId,
    ticket: RestoreTicket,
) -> Result<RestoreTick, RestoreFailure> {
    let lease = match repo.claim(tenant, ticket).await? {
        RestoreClaim::Idle => return Ok(RestoreTick::Idle),
        RestoreClaim::Disabled => return Ok(RestoreTick::Disabled),
        RestoreClaim::Denied => return Ok(RestoreTick::Denied),
        RestoreClaim::Claimed(lease) => lease,
    };
    let segment = match source.load(tenant, lease.object).await {
        Ok(segment) => segment,
        Err(error) => {
            repo.abandon(&lease).await?;
            return Err(error);
        }
    };
    Ok(if repo.complete(&lease, &segment).await? {
        RestoreTick::Ready
    } else {
        RestoreTick::Fenced
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tickets_reject_nonpositive_identities() {
        for (manifest, generation) in [(0, 1), (1, 0), (-1, 1), (1, -1)] {
            assert!(RestoreTicket::new(manifest, generation).is_err());
        }
        assert!(RestoreTicket::new(1, 1).is_ok());
    }
}

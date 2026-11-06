//! Trusted archive publication; a manifest is coverage evidence, never deletion authority.
use crate::StoredPropertyChange;
use fleetiq_domain::TenantId;
use std::{error::Error, future::Future};
/// Verified whole-object identity, separate from logical codec checksums.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchiveObject {
    digest: [u8; 32],
    bytes: i64,
}
impl ArchiveObject {
    /// Reconstructs bounded evidence supplied by the trusted verifying adapter.
    /// # Errors
    /// Rejects empty or oversized files; construction alone proves no storage existence.
    pub fn new(digest: [u8; 32], bytes: i64) -> Result<Self, ArchiveFailure> {
        if !(1..=2_097_152).contains(&bytes) {
            return Err(ArchiveFailure);
        }
        Ok(Self { digest, bytes })
    }
    /// Complete-object SHA-256.
    pub const fn digest(self) -> [u8; 32] {
        self.digest
    }
    /// Exact verified object length.
    pub const fn bytes(self) -> i64 {
        self.bytes
    }
}
/// One frozen tenant selection and database-owned attempt fence.
#[derive(Clone)]
pub struct ArchiveLease {
    /// Tenant owning all selected evidence and object bytes.
    pub tenant: TenantId,
    /// Stable logical publication identity across retries.
    pub job_id: i64,
    /// Monotone claim generation.
    pub generation: i64,
    /// Tenant-scoped budget reservation for this attempt.
    pub reservation: i64,
    /// Frozen ordered membership, at most sixteen interpretations.
    pub changes: Vec<StoredPropertyChange>,
}
/// Explicit admission state, independent of empty history.
pub enum ArchiveClaim {
    /// No unassigned or expired pending selection.
    Idle,
    /// No enabled lifecycle policy or inactive tenant.
    Disabled,
    /// Tenant budget cannot admit this turn.
    Denied,
    /// One durable selection admitted for bounded processing.
    Claimed(ArchiveLease),
}
/// Published immutable coverage, visible only after object verification and commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveManifest {
    /// Stable publication identity, scoped by the caller's tenant.
    pub job_id: i64,
    /// Generation that committed verification.
    pub generation: i64,
    /// Complete immutable object descriptor.
    pub object: ArchiveObject,
    /// Number of exact interpretation identities in frozen membership.
    pub members: i64,
}
/// Redacted archive failure; durable state remains retryable after expiry.
#[derive(Debug)]
pub struct ArchiveFailure;
impl std::fmt::Display for ArchiveFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("archive turn failed")
    }
}
impl Error for ArchiveFailure {}
/// History-owned durable selection and publication port.
pub trait ArchiveWorkRepository: Send + Sync {
    /// Claims under a thirty-second database lease and tenant reservation.
    fn claim(
        &self,
        tenant: &TenantId,
    ) -> impl Future<Output = Result<ArchiveClaim, ArchiveFailure>> + Send;
    /// Publishes only the current unexpired generation; exact completion retries converge.
    fn complete(
        &self,
        lease: &ArchiveLease,
        object: ArchiveObject,
    ) -> impl Future<Output = Result<bool, ArchiveFailure>> + Send;
    /// Ends a failed attempt and releases only its own reservation.
    fn abandon(
        &self,
        lease: &ArchiveLease,
    ) -> impl Future<Output = Result<(), ArchiveFailure>> + Send;
}
/// Adapter verifies complete bytes before returning a descriptor; no vendor types cross here.
pub trait ArchiveSegmentStore: Send + Sync {
    /// Stores/verifies the exact ordered selection, including tenant and codec profile.
    fn store(
        &self,
        tenant: &TenantId,
        changes: &[StoredPropertyChange],
    ) -> impl Future<Output = Result<ArchiveObject, ArchiveFailure>> + Send;
}
/// Result of an explicit bounded archive turn.
#[derive(Debug, Eq, PartialEq)]
pub enum ArchiveTick {
    /// No work.
    Idle,
    /// Policy disabled or tenant inactive.
    Disabled,
    /// Budget admission denied.
    Denied,
    /// Verified manifest committed.
    Published,
    /// Lease expired or another generation owns publication.
    Fenced,
}
/// Coordinates verification before publication; caller enforces a twenty-second turn limit.
/// # Errors
/// Returns redacted selection, verification or publication failures; leases recover by expiry.
pub async fn archive_once<R: ArchiveWorkRepository, S: ArchiveSegmentStore>(
    repo: &R,
    store: &S,
    tenant: &TenantId,
) -> Result<ArchiveTick, ArchiveFailure> {
    let lease = match repo.claim(tenant).await? {
        ArchiveClaim::Idle => return Ok(ArchiveTick::Idle),
        ArchiveClaim::Disabled => return Ok(ArchiveTick::Disabled),
        ArchiveClaim::Denied => return Ok(ArchiveTick::Denied),
        ArchiveClaim::Claimed(lease) => lease,
    };
    let object = match store.store(tenant, &lease.changes).await {
        Ok(object) => object,
        Err(error) => {
            repo.abandon(&lease).await?;
            return Err(error);
        }
    };
    Ok(if repo.complete(&lease, object).await? {
        ArchiveTick::Published
    } else {
        ArchiveTick::Fenced
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whole_object_evidence_has_exact_positive_bounds() {
        for bytes in [i64::MIN, -1, 0, 2_097_153, i64::MAX] {
            assert!(ArchiveObject::new([0; 32], bytes).is_err());
        }
        assert_eq!(
            ArchiveObject::new([1; 32], 2_097_152).unwrap().bytes(),
            2_097_152
        );
    }
}

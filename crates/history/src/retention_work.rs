//! Explicit retention of verified hot projection payloads; source/identity anchors remain.
use crate::{archive_work::ArchiveObject, restore::ArchiveRestoreSource};
use fleetiq_domain::TenantId;
use std::future::Future;
/// Redacted failure; an uncertain transaction is reconciled by retrying the same target.
#[derive(Debug)]
pub struct RetentionFailure;
impl std::fmt::Display for RetentionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("retention failed")
    }
}
impl std::error::Error for RetentionFailure {}
/// Exact operator-selected manifest and expected policy; preview tokens are never accepted.
pub struct RetentionTarget {
    tenant: TenantId,
    manifest: i64,
    revision: i64,
}
impl RetentionTarget {
    /// Validates positive identifiers, without approving destructive execution.
    /// # Errors
    /// Rejects invalid manifest/policy revisions.
    pub fn new(tenant: TenantId, manifest: i64, revision: i64) -> Result<Self, RetentionFailure> {
        if manifest <= 0 || revision <= 0 {
            return Err(RetentionFailure);
        }
        Ok(Self {
            tenant,
            manifest,
            revision,
        })
    }
    /// Tenant owning every anchor and archive member.
    pub const fn tenant(&self) -> &TenantId {
        &self.tenant
    }
    /// Immutable manifest identifier.
    pub const fn manifest(&self) -> i64 {
        self.manifest
    }
    /// Exact approved policy revision.
    pub const fn revision(&self) -> i64 {
        self.revision
    }
}
/// Durable bounded outcome, not physical disk savings.
#[derive(Debug, Eq, PartialEq)]
pub enum RetentionTick {
    /// No matching explicit approval, or tenant/policy disabled.
    Disabled,
    /// Age, archive or dependency protections prevent execution.
    Protected,
    /// Current tenant concurrency/byte allowance cannot admit a bounded turn.
    Denied,
    /// All selected payloads were reclaimed atomically and audited.
    Reclaimed(u32),
    /// A prior committed receipt reconciles this exact retry.
    AlreadyReclaimed(u32),
}
/// Outcome before object I/O; terminal retries do not need the provider.
pub enum RetentionCheck {
    /// No new mutation is attempted.
    Finished(RetentionTick),
    /// Whole immutable object requiring a fresh bounded verified read.
    Verify(ArchiveObject),
}
/// Trusted history-owned atomic lifecycle boundary; no caller-supplied deletion permit.
pub trait RetentionRepository: Send + Sync {
    /// Resolves explicit approval/current state or a previous immutable receipt.
    fn inspect(
        &self,
        target: &RetentionTarget,
    ) -> impl Future<Output = Result<RetentionCheck, RetentionFailure>> + Send;
    /// Freshly loads through the trusted verifying source, admits bounded work, then rechecks protections under write exclusion.
    fn reclaim<S: ArchiveRestoreSource>(
        &self,
        target: &RetentionTarget,
        source: &S,
    ) -> impl Future<Output = Result<RetentionTick, RetentionFailure>> + Send;
}
/// Performs one explicit verified manifest turn; the composition root imposes its deadline.
/// # Errors
/// Fails closed on provider, codec, membership or database failure; no partial outcome commits.
pub async fn reclaim_once<R: RetentionRepository, S: ArchiveRestoreSource>(
    repo: &R,
    source: &S,
    target: &RetentionTarget,
) -> Result<RetentionTick, RetentionFailure> {
    repo.reclaim(target, source).await
}

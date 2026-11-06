//! Read-only candidate assessment. Anchors and current immutable rows are never deletion targets.
use super::EvidenceProtection;
/// Known consumer families; unresolved/exhausted work remains protected too.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetentionConsumer {
    /// Unfinished protocol replay.
    Replay,
    /// Unfinished semantic interpretation.
    Semantic,
    /// Calculator scheduling/preparation/work may still read hot history.
    Calculator,
    /// Pending, live or in-flight restore work.
    Restore,
    /// Unprocessed projection work still needs hot payloads.
    Projection,
}
impl RetentionConsumer {
    /// Stable non-sensitive diagnostic category.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Replay => "replay",
            Self::Semantic => "semantic",
            Self::Calculator => "calculator",
            Self::Restore => "restore",
            Self::Projection => "projection",
        }
    }
}
/// One bounded interpretation assessment, containing no telemetry values.
pub struct RetentionCandidate {
    /// Opaque complete-identity digest, not an authorization grant.
    pub id: String,
    /// Verified manifest lease end; a conservative floor after source visibility.
    pub age_floor_ms: Option<i64>,
    /// Valid committed publication receipt, not a fresh provider durability check.
    pub archive_verified: bool,
    /// Conservative tenant-wide consumer holds.
    pub consumers: Vec<RetentionConsumer>,
    /// Payload blockers; identity/lineage anchors are retained separately in every case.
    pub protections: Vec<EvidenceProtection>,
}
impl RetentionCandidate {
    /// Assesses policy, conservative age, archive receipt and every consumer hold.
    /// Source/device timestamps never establish age. Invalid/missing inputs fail closed.
    pub fn assess(
        id: String,
        age_floor_ms: Option<i64>,
        observed_at_ms: i64,
        hot_days: Option<i32>,
        archive_verified: bool,
        consumers: Vec<RetentionConsumer>,
    ) -> Self {
        let mut protections = Vec::new();
        let configured = hot_days.filter(|d| *d > 0);
        if configured.is_none() {
            protections.push(EvidenceProtection::PolicyDisabled);
        }
        let verified = archive_verified && age_floor_ms.is_some();
        if !verified {
            protections.push(EvidenceProtection::UnverifiedArchive);
        }
        if let (Some(days), Some(floor)) = (configured, age_floor_ms)
            && i128::from(observed_at_ms) - i128::from(floor) < i128::from(days) * 86_400_000
        {
            protections.push(EvidenceProtection::WithinHotRetention);
        }
        if !consumers.is_empty() {
            protections.push(EvidenceProtection::ActiveConsumer);
        }
        Self {
            id,
            age_floor_ms,
            archive_verified: verified,
            consumers,
            protections,
        }
    }
    /// Eligible only as a future payload candidate; never authority to mutate an immutable row.
    pub fn payload_eligible(&self) -> bool {
        self.protections.is_empty()
    }
    /// Potential logical bytes of three BIGINT projection fields, not disk reclamation.
    pub fn estimated_reclaimable_bytes(&self) -> u64 {
        if self.payload_eligible() { 24 } else { 0 }
    }
}
/// Consistent bounded sample; not a whole-tenant inventory or executor permit.
pub struct RetentionPreview {
    /// Database snapshot time, in signed Unix milliseconds.
    pub observed_at_ms: i64,
    /// Exact latest policy revision; zero means absent. Disabled revisions remain explicit.
    pub policy_revision: i64,
    /// At most one hundred interpretations in stable complete-identity order.
    pub candidates: Vec<RetentionCandidate>,
    /// A look-ahead found more interpretations than the sample limit.
    pub has_more: bool,
}

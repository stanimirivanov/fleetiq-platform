//! Non-sensitive bounded preview transport. Tokens are public context, never execution permits.
use fleetiq_history::lifecycle::{EvidenceProtection, retention::RetentionCandidate};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Parameters {
    pub(super) preview: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ContextToken {
    pub(super) kind: String,
    pub(super) tenant: String,
    pub(super) revision: i64,
    pub(super) observed: i64,
    pub(super) expires: i64,
    pub(super) fingerprint: String,
}
#[derive(Serialize)]
pub(super) struct CandidateBody {
    candidate_id: String,
    age_floor_ms: Option<String>,
    archive_verified: bool,
    active_consumers: Vec<&'static str>,
    payload_eligible: bool,
    estimated_reclaimable_bytes: String,
    protections: Vec<&'static str>,
}
impl From<&RetentionCandidate> for CandidateBody {
    fn from(c: &RetentionCandidate) -> Self {
        Self {
            candidate_id: c.id.clone(),
            age_floor_ms: c.age_floor_ms.map(|n| n.to_string()),
            archive_verified: c.archive_verified,
            active_consumers: c.consumers.iter().map(|c| c.code()).collect(),
            payload_eligible: c.payload_eligible(),
            estimated_reclaimable_bytes: c.estimated_reclaimable_bytes().to_string(),
            protections: c
                .protections
                .iter()
                .map(|p| match p {
                    EvidenceProtection::PolicyDisabled => "policy_disabled",
                    EvidenceProtection::WithinHotRetention => "hot_retention",
                    EvidenceProtection::UnverifiedArchive => "unverified_archive",
                    EvidenceProtection::ActiveConsumer => "active_consumer",
                    EvidenceProtection::Lineage => "lineage",
                    EvidenceProtection::Idempotency => "idempotency",
                    EvidenceProtection::VerifiedUnreferencedPayload => {
                        "verified_unreferenced_payload"
                    }
                })
                .collect(),
        }
    }
}
#[derive(Serialize)]
pub(super) struct PreviewBody {
    pub(super) observed_at_ms: String,
    pub(super) expires_at_ms: String,
    pub(super) policy_revision: String,
    pub(super) preview_token: String,
    pub(super) sample_scope: &'static str,
    pub(super) has_more: bool,
    pub(super) consumer_scope: &'static str,
    pub(super) age_method: &'static str,
    pub(super) estimate_method: &'static str,
    pub(super) estimated_reclaimable_bytes: String,
    pub(super) physical_reclaimable_bytes: &'static str,
    pub(super) retained_anchors: [&'static str; 2],
    pub(super) reclamation_supported: bool,
    pub(super) candidates: Vec<CandidateBody>,
}
pub(super) fn fingerprint(
    revision: i64,
    more: bool,
    candidates: &[CandidateBody],
) -> Result<String, serde_json::Error> {
    let digest = Sha256::digest(serde_json::to_vec(&(revision, more, candidates))?);
    let mut result = String::with_capacity(64);
    for byte in digest {
        write!(result, "{byte:02x}").expect("String formatting");
    }
    Ok(result)
}

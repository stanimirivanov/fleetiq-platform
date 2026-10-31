//! Bounded diagnostic evidence, never a claim about worker liveness or total backlog.
/// One bounded queue/rule-prefix observation. Counts describe observations, never total queue size.
#[derive(Clone, Debug)]
pub struct WorkQueueEvidence {
    /// Closed queue name supplied by the read adapter.
    pub kind: &'static str,
    /// Number of sampled positions or selected alert rules, at most 64.
    pub sampled: u32,
    /// Eligible sampled positions (alert rules contribute their next evidence only).
    pub eligible: u32,
    /// Sampled positions with a currently unexpired database lease.
    pub leased: u32,
    /// Terminal or expired final-attempt sampled positions.
    pub failed: u32,
    /// Sampled positions blocked by a predecessor, release policy or selection.
    pub blocked: u32,
    /// Sampled positions awaiting retry or refresh time.
    pub deferred: u32,
    /// Maximum attempts observed; zero does not imply no previous processing.
    pub max_attempts: u32,
    /// Oldest sampled eligible receipt/window/evidence/due time; not a global minimum.
    pub oldest_sampled_eligible_at_ms: Option<i64>,
    /// Closed failure categories observed in the sample; no upstream exception text.
    pub failure_categories: Vec<&'static str>,
    /// More positions/rules exist outside the deterministic identity prefix.
    pub truncated: bool,
}
impl WorkQueueEvidence {
    /// Starts a queue observation without inventing any health evidence.
    pub fn empty(kind: &'static str) -> Self {
        Self {
            kind,
            sampled: 0,
            eligible: 0,
            leased: 0,
            failed: 0,
            blocked: 0,
            deferred: 0,
            max_attempts: 0,
            oldest_sampled_eligible_at_ms: None,
            failure_categories: Vec::new(),
            truncated: false,
        }
    }
}
/// A single read-only database snapshot across the four work families.
#[derive(Debug)]
pub struct WorkDiagnostics {
    /// Database transaction time in Unix milliseconds.
    pub observed_at_ms: i64,
    /// Exactly four bounded observations, including empty queues.
    pub queues: Vec<WorkQueueEvidence>,
}

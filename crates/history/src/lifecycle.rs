//! Explicit lifecycle settings; no setting starts work or authorizes deletion.
#[cfg(test)]
mod tests;

/// Validated operator-supplied retention and work limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LifecyclePolicy {
    hot_days: i32,
    archive_days: i32,
    max_jobs: i32,
    max_bytes: i64,
}
impl LifecyclePolicy {
    /// Validates positive limits and archive retention no shorter than hot retention.
    /// # Errors
    /// Returns an error for contradictory durations or nonpositive limits.
    pub fn new(
        hot_days: i32,
        archive_days: i32,
        max_jobs: i32,
        max_bytes: i64,
    ) -> Result<Self, InvalidLifecyclePolicy> {
        if hot_days <= 0 || archive_days < hot_days || max_jobs <= 0 || max_bytes <= 0 {
            return Err(InvalidLifecyclePolicy);
        }
        Ok(Self {
            hot_days,
            archive_days,
            max_jobs,
            max_bytes,
        })
    }
    /// Hot retention duration, in days.
    pub const fn hot_days(self) -> i32 {
        self.hot_days
    }
    /// Archive retention duration, in days; never an automatic deletion instruction.
    pub const fn archive_days(self) -> i32 {
        self.archive_days
    }
    /// Maximum concurrent lifecycle jobs.
    pub const fn max_jobs(self) -> i32 {
        self.max_jobs
    }
    /// Maximum bytes reserved for lifecycle work.
    pub const fn max_bytes(self) -> i64 {
        self.max_bytes
    }
}
/// Invalid lifecycle limits without user data in diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidLifecyclePolicy;
impl std::fmt::Display for InvalidLifecyclePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid lifecycle policy")
    }
}
impl std::error::Error for InvalidLifecyclePolicy {}
/// Absent configuration is disabled; no implicit retention policy is selected.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LifecycleSettings {
    /// Preserve current behavior and all evidence.
    #[default]
    Disabled,
    /// Explicit limits for later lifecycle consumers.
    Configured(LifecyclePolicy),
}
/// Reasons a future retention selector must distinguish, not deletion authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceProtection {
    /// No configured retention policy authorizes even candidate selection.
    PolicyDisabled,
    /// Conservative hot-age floor has not elapsed.
    WithinHotRetention,
    /// Original attribution and provenance anchors remain retained.
    Lineage,
    /// Durable retry/duplicate identities remain retained.
    Idempotency,
    /// Replay, semantic, calculator or restore work still references the evidence.
    ActiveConsumer,
    /// Archive verification/publication has not completed.
    UnverifiedArchive,
    /// Payload with verified archive and no consumer or anchor protection.
    VerifiedUnreferencedPayload,
}
impl EvidenceProtection {
    /// Reports candidate eligibility only; execution must recheck current protections.
    pub const fn permits_reclamation(self) -> bool {
        matches!(self, Self::VerifiedUnreferencedPayload)
    }
}

/// Outcome of transactional work admission, independent of query results.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetAdmission {
    /// No configured limits; retain existing behavior.
    Disabled,
    /// Current slot or reserved-byte allowance cannot admit this work.
    Denied,
    /// Durable reservation identity, scoped to its tenant; not an authorization token.
    Reserved(i64),
}

/// Conservative read-only retention assessment models.
pub mod retention;
#[cfg(test)]
#[path = "lifecycle/retention_tests.rs"]
mod retention_tests;

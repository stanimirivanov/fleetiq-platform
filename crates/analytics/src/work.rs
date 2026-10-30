//! Fixed-window schedules and generation-fenced calculator work.
use crate::{CalculatorConfigurationId, CalculatorRef, CalculatorRunRequest, CalculatorWindow};
use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};

/// Immutable trusted schedule; HTTP activation and cron are separate capabilities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalculatorSchedule {
    /// Tenant owning the schedule and all references.
    pub tenant: TenantId,
    /// Stable schedule identity (same validation as configuration identifiers).
    pub id: CalculatorConfigurationId,
    /// Exact admitted first-party release.
    pub calculator: CalculatorRef,
    /// Immutable parameter identity retained in every evaluation.
    pub configuration: CalculatorConfigurationId,
    /// Semantic input scope.
    pub scope: AssetId,
    /// Output target with registered property membership.
    pub target: AssetId,
    /// First inclusive window start; catch-up never invents an earlier interval.
    pub first_start: UnixTimeMillis,
    /// Nonzero window size, at most seven days.
    pub window_ms: u32,
    /// Allowed lateness, at most seven days.
    pub lateness_ms: u32,
    /// Required sample count, 1..=1024.
    pub minimum_samples: u16,
}
impl CalculatorSchedule {
    /// Checks bounds and overflow without reading a clock or database.
    pub fn is_valid(&self) -> bool {
        let Some(end) = self
            .first_start
            .get()
            .checked_add(i64::from(self.window_ms))
        else {
            return false;
        };
        let Some(finalize) = end.checked_add(i64::from(self.lateness_ms)) else {
            return false;
        };
        (1..=604_800_000).contains(&self.window_ms)
            && self.lateness_ms <= 604_800_000
            && (1..=1024).contains(&self.minimum_samples)
            && CalculatorWindow::new(
                self.first_start,
                UnixTimeMillis::new(end),
                UnixTimeMillis::new(finalize),
            )
            .is_ok()
    }
}
/// One database-clock claim; original request/preparation is shared by retries.
#[derive(Clone, Debug)]
pub struct CalculatorWorkLease {
    /// Original immutable evaluation request created by scheduling.
    pub request: CalculatorRunRequest,
    /// Monotonically increasing attempt fence, from one through three.
    pub generation: i32,
}

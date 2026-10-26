//! One bounded scheduling turn; process composition owns polling and cancellation.
use super::{ReplayEffects, ReplayRepository, ReplayWork};
use std::future::Future;
/// Execution outcomes distinguish poison input from retryable infrastructure failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayFailure {
    /// Protocol, admission, policy or validated output rejection.
    Rejected,
    /// Child startup, deadline, cancellation or artifact-store unavailability.
    Unavailable,
}
/// Consumed execution port; historical work never exposes transport replies.
pub trait ReplayExecutor: Send + Sync {
    /// Executes exact stored bytes and state under pinned replay policy.
    fn execute(
        &self,
        work: &ReplayWork,
    ) -> impl Future<Output = Result<ReplayEffects, ReplayFailure>> + Send;
}
/// Safe scheduling result; no payload or guest diagnostics enter routine reporting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayTick {
    /// No eligible work exists.
    Idle,
    /// Outputs and successor checkpoint committed.
    Completed,
    /// Lease expired or another generation owns the work.
    LostLease,
    /// Protocol rejection quarantined; the session remains blocked.
    Poisoned,
    /// Infrastructure failure left the lease for bounded expiry/retry.
    Deferred,
}
/// Coordinates durable claims, bounded execution and atomic completion.
pub struct ReplayWorker<'a, R, E> {
    repository: &'a R,
    executor: &'a E,
}
impl<'a, R: ReplayRepository, E: ReplayExecutor> ReplayWorker<'a, R, E> {
    /// Borrows capability ports; owns no socket, timer, task or database connection.
    pub const fn new(repository: &'a R, executor: &'a E) -> Self {
        Self {
            repository,
            executor,
        }
    }
    /// Runs at most one leased turn. Cancellation leaves recoverable durable input.
    /// # Errors
    /// Reports repository failures; ambiguous completion must be retried by identity.
    pub async fn run_once(&self) -> Result<ReplayTick, R::Error> {
        let Some(lease) = self.repository.claim().await? else {
            return Ok(ReplayTick::Idle);
        };
        match self.executor.execute(&lease.work).await {
            Ok(effects) => match effects.batch(&lease.work) {
                Ok((batch, state)) => {
                    Ok(if self.repository.complete(&lease, &batch, &state).await? {
                        ReplayTick::Completed
                    } else {
                        ReplayTick::LostLease
                    })
                }
                Err(_) => self.poison(&lease).await,
            },
            Err(ReplayFailure::Rejected) => self.poison(&lease).await,
            Err(ReplayFailure::Unavailable) => Ok(ReplayTick::Deferred),
        }
    }
    async fn poison(&self, lease: &super::ReplayLease) -> Result<ReplayTick, R::Error> {
        Ok(if self.repository.poison(lease).await? {
            ReplayTick::Poisoned
        } else {
            ReplayTick::LostLease
        })
    }
}

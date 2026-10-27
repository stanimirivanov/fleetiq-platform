//! Consumer-owned durable semantic work and recovery boundary.
use crate::StoredObservationRequest;
use fleetiq_domain::SignalBinding;
use fleetiq_semantics::AssetPropertyChange;
use std::{error::Error, future::Future};

mod worker;
pub use worker::{SemanticWorker, SemanticWorkerError};

/// One claimed source and its immutable interpretation choices.
/// Formatting deliberately omits source and binding identities.
#[derive(Clone)]
pub struct SemanticLease {
    /// Exact durable source, knowledge cutoff and event-time basis.
    pub request: StoredObservationRequest,
    /// Database-owned attempt fence; a stale generation cannot write.
    pub generation: i32,
    /// Previously committed selection, loaded by exact immutable revision.
    pub selected: Option<SignalBinding>,
}
impl std::fmt::Debug for SemanticLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticLease")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}
/// Retained diagnostic category; raw values and definitions stay at their source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticUnresolved {
    /// No effective binding in the frozen snapshot.
    NoBinding,
    /// Multiple competing bindings with no authority rule.
    Ambiguous,
    /// The exact target property definition is unavailable.
    MissingDefinition,
    /// Decoded and target value kinds differ.
    ValueKind,
    /// Unit conversion requires an explicit policy.
    Unit,
}
/// Outcome of a bounded worker turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticTick {
    /// No currently claimable work (including suspended tenants).
    Idle,
    /// History and terminal progress committed together.
    Completed,
    /// Unresolved configuration retained without dropping its source.
    Unresolved,
    /// Ownership expired or was replaced; no side effect was authorized.
    Fenced,
}
/// Durable scheduling port owned by history, not ingestion or transport.
///
/// Implementations fence all writes by active tenant, exact source, generation
/// and unexpired database lease. Selection commits before append. Completion
/// appends the exact selected interpretation and progress in one transaction.
/// Failed/ambiguous calls leave durable state for lease-based recovery.
pub trait SemanticWorkRepository: Send + Sync {
    /// Storage or invalid durable-state failure.
    type Error: Error + Send + Sync + 'static;
    /// Advances pre-migration discovery by at most 32 source rows.
    fn seed(&self) -> impl Future<Output = Result<(), Self::Error>> + Send;
    /// Claims one pending source; expired final attempts become retained exhausted work.
    fn claim(&self) -> impl Future<Output = Result<Option<SemanticLease>, Self::Error>> + Send;
    /// Commits the exact binding selection before any history append.
    fn pin(
        &self,
        lease: &SemanticLease,
        change: &AssetPropertyChange,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;
    /// Atomically appends selected history and marks this exact work complete.
    fn complete(
        &self,
        lease: &SemanticLease,
        change: &AssetPropertyChange,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;
    /// Stores a closed unresolved reason; ordinary polling never reinterprets it.
    fn retain(
        &self,
        lease: &SemanticLease,
        reason: SemanticUnresolved,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;
}

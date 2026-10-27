//! Application-owned durability contract for semantic property history.
//!
//! Successfully resolved property changes are immutable facts. The semantic
//! resolver remains read-only; a fenced worker appends its output here
//! after decoded observations are durable. An exact read port exposes stored
//! interpretations without selecting a latest value.

use std::{error::Error, future::Future};

use fleetiq_semantics::AssetPropertyChange;

mod observation_interpretation_page;
mod processing;
mod property_change_view;
mod property_history;
pub use property_history::{
    InvalidPropertyHistoryQuery, PropertyHistoryCursor, PropertyHistoryPage, PropertyHistoryQuery,
    PropertyHistoryService, PropertyHistoryView,
};
mod stored_processing;

pub use observation_interpretation_page::{
    InvalidObservationInterpretationPageQuery, ObservationInterpretationPage,
    ObservationInterpretationPageQuery,
};
pub use processing::{
    SemanticHistoryError, SemanticHistoryOutcome, SemanticHistoryProcessor, SemanticHistoryRetry,
};
pub use property_change_view::{
    AssetPropertyChangeView, InvalidStoredPropertyChange, StoredPropertyChange,
};
pub use stored_processing::{
    DecodedObservationView, StoredObservationRequest, StoredSemanticHistoryError,
    StoredSemanticHistoryProcessor,
};

/// Result of durably appending one semantic interpretation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyChangeAppendOutcome {
    /// This interpretation was appended for the first time.
    Appended,
    /// The same interpretation was already durable.
    AlreadyPresent,
}

/// Append-only boundary for asset-property change candidates.
///
/// The exact decoded observation and binding revision must already be durable
/// and belong to the same tenant. An implementation verifies the supplied
/// immutable provenance against those facts before recording the interpretation.
/// An ambiguous storage error may mean the append committed; callers retain
/// the borrowed change and can retry it unchanged.
pub trait AssetPropertyChangeLog: Send + Sync {
    /// Infrastructure or invalid-reference failure.
    type Error: Error + Send + Sync + 'static;

    /// Persists one interpretation or identifies an exact retry.
    ///
    /// # Errors
    ///
    /// Returns an implementation error if provenance disagrees with durable
    /// source facts, the change identity is reused with different content, or
    /// durability cannot be confirmed. No partial semantic fact is exposed.
    fn append<'a>(
        &'a self,
        change: &'a AssetPropertyChange,
    ) -> impl Future<Output = Result<PropertyChangeAppendOutcome, Self::Error>> + Send + 'a;
}

mod work;
pub use work::{
    SemanticLease, SemanticTick, SemanticUnresolved, SemanticWorkRepository, SemanticWorker,
    SemanticWorkerError,
};

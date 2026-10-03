//! Exact durable-observation lookup followed by semantic history processing.

use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{TenantId, UnixTimeMillis};
use fleetiq_semantics::{EventTimeBasis, PropertyDefinitionView, SignalBindingView};
use fleetiq_telemetry::{DecodedObservation, DecodedObservationId};

use crate::{
    AssetPropertyChangeLog, SemanticHistoryError, SemanticHistoryOutcome, SemanticHistoryProcessor,
};

/// Reads one immutable decoded observation for a tenant and exact decoder output.
///
/// Absence is distinct from a storage failure. Implementations must never
/// select another tenant or decoder release when the exact identity is absent.
pub trait DecodedObservationView: Send + Sync {
    /// Infrastructure or invalid persisted-data failure.
    type Error: Error + Send + Sync + 'static;

    /// Looks up one complete, validated observation without reading raw bytes.
    fn find<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        id: &'a DecodedObservationId,
    ) -> impl Future<Output = Result<Option<DecodedObservation>, Self::Error>> + Send + 'a;
}

/// Explicit identity and time choices for one durable semantic processing call.
#[derive(Clone, Eq, PartialEq)]
pub struct StoredObservationRequest {
    tenant_id: TenantId,
    id: DecodedObservationId,
    known_at: UnixTimeMillis,
    basis: EventTimeBasis,
}

impl StoredObservationRequest {
    /// Selects an exact tenant-owned output and the attribution time policy.
    pub const fn new(
        tenant_id: TenantId,
        id: DecodedObservationId,
        known_at: UnixTimeMillis,
        basis: EventTimeBasis,
    ) -> Self {
        Self {
            tenant_id,
            id,
            known_at,
            basis,
        }
    }

    /// Returns the tenant that owns the decoded observation.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the exact message, decoder release, and output position.
    pub const fn id(&self) -> &DecodedObservationId {
        &self.id
    }

    /// Returns the inclusive recorded-time knowledge cutoff.
    pub const fn known_at(&self) -> UnixTimeMillis {
        self.known_at
    }

    /// Returns the preserved timestamp selected as event time.
    pub const fn basis(&self) -> EventTimeBasis {
        self.basis
    }
}

impl fmt::Debug for StoredObservationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StoredObservationRequest")
            .finish_non_exhaustive()
    }
}

/// Processes one exact decoded output already committed by ingestion.
///
/// The borrowed request remains with the caller after lookup failures. Once
/// loaded, the ordinary processor owns the observation and retains its retry
/// value on failure. No polling, scheduling, or batch policy is implied.
#[derive(Clone, Debug)]
pub struct StoredSemanticHistoryProcessor<O, B, D, L> {
    observations: O,
    processor: SemanticHistoryProcessor<B, D, L>,
}

impl<O, B, D, L> StoredSemanticHistoryProcessor<O, B, D, L> {
    /// Composes an exact decoded-observation view and the semantic processor.
    pub const fn new(observations: O, processor: SemanticHistoryProcessor<B, D, L>) -> Self {
        Self {
            observations,
            processor,
        }
    }
}

impl<O, B, D, L> StoredSemanticHistoryProcessor<O, B, D, L>
where
    O: DecodedObservationView,
    B: SignalBindingView,
    D: PropertyDefinitionView,
    L: AssetPropertyChangeLog,
{
    /// Loads and interprets one exact durable output.
    ///
    /// Returns `None` only when the requested observation does not exist in
    /// that tenant. Unresolved attribution is a present semantic outcome.
    /// Calling this again with the same request is safe after an exact append;
    /// an ambiguous append must instead retry the returned change unchanged.
    ///
    /// # Errors
    ///
    /// Returns a lookup or identity-mismatch error before semantic processing,
    /// or the ordinary processor error with owned retry input after a valid read.
    pub async fn process(
        &self,
        request: &StoredObservationRequest,
    ) -> Result<
        Option<SemanticHistoryOutcome>,
        StoredSemanticHistoryError<O::Error, B::Error, D::Error, L::Error>,
    > {
        let Some(observation) = self
            .observations
            .find(request.tenant_id(), request.id())
            .await
            .map_err(StoredSemanticHistoryError::Lookup)?
        else {
            return Ok(None);
        };
        if observation.tenant_id() != request.tenant_id() || observation.id() != request.id() {
            return Err(StoredSemanticHistoryError::SourceMismatch);
        }
        self.processor
            .process(observation, request.known_at(), request.basis())
            .await
            .map(Some)
            .map_err(StoredSemanticHistoryError::Processing)
    }
}

/// Failure before or during durable-observation semantic processing.
pub enum StoredSemanticHistoryError<O, B, D, L> {
    /// The exact decoded observation could not be read.
    Lookup(O),
    /// The view returned a different tenant or decoded-output identity.
    SourceMismatch,
    /// Semantic resolution or history append failed after the read.
    Processing(SemanticHistoryError<B, D, L>),
}

impl<O, B, D, L> fmt::Debug for StoredSemanticHistoryError<O, B, D, L> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::Lookup(_) => "lookup failure",
            Self::SourceMismatch => "source mismatch",
            Self::Processing(_) => "processing failure",
        };
        formatter
            .debug_struct("StoredSemanticHistoryError")
            .field("kind", &kind)
            .finish_non_exhaustive()
    }
}

impl<O, B, D, L> fmt::Display for StoredSemanticHistoryError<O, B, D, L> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lookup(_) => formatter.write_str("durable decoded observation lookup failed"),
            Self::SourceMismatch => {
                formatter.write_str("durable decoded observation identity differs from the request")
            }
            Self::Processing(_) => formatter.write_str("semantic history processing failed"),
        }
    }
}

impl<O, B, D, L> Error for StoredSemanticHistoryError<O, B, D, L>
where
    O: Error + 'static,
    B: Error + 'static,
    D: Error + 'static,
    L: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Lookup(source) => Some(source),
            Self::SourceMismatch => None,
            Self::Processing(source) => Some(source),
        }
    }
}

#[cfg(test)]
mod tests;

//! One-observation semantic attribution and durable history orchestration.

use std::{error::Error, fmt};

use fleetiq_domain::UnixTimeMillis;
use fleetiq_semantics::{
    AssetPropertyChange, AssetPropertyChangeError, EventTimeBasis, PropertyDefinitionView,
    ResolutionContext, ResolutionError, ResolutionOutcome, SignalBindingResolver,
    SignalBindingView, UnresolvedObservation,
};
use fleetiq_telemetry::DecodedObservation;

use crate::{AssetPropertyChangeLog, PropertyChangeAppendOutcome};

/// Coordinates one already durable decoded observation into semantic history.
///
/// The caller explicitly selects a preserved timestamp as event time and an
/// inclusive recorded-time knowledge cutoff. The selected event time also
/// becomes the binding's physical effective-time cutoff, preventing history
/// from claiming a value at an instant different from its attribution view.
/// The history adapter verifies the supplied observation against durable facts.
#[derive(Clone, Debug)]
pub struct SemanticHistoryProcessor<B, D, L> {
    resolver: SignalBindingResolver<B, D>,
    log: L,
}

impl<B, D, L> SemanticHistoryProcessor<B, D, L> {
    /// Composes the semantic read policy with one append-only history port.
    pub const fn new(resolver: SignalBindingResolver<B, D>, log: L) -> Self {
        Self { resolver, log }
    }
}

impl<B, D, L> SemanticHistoryProcessor<B, D, L>
where
    B: SignalBindingView,
    D: PropertyDefinitionView,
    L: AssetPropertyChangeLog,
{
    /// Resolves and durably appends one decoded observation.
    ///
    /// An unresolved observation remains a structured outcome with its source
    /// value and context; it is never appended or silently discarded. A
    /// repository failure retains the observation for re-resolution. A failed
    /// append retains the exact change so retry cannot select a newer binding.
    ///
    /// # Errors
    ///
    /// Returns [`SemanticHistoryError::MissingDeviceRecordedTime`] before any
    /// repository access when device time was selected but absent. Operational
    /// read or append failures preserve their causes and retry input.
    pub async fn process(
        &self,
        observation: DecodedObservation,
        known_at: UnixTimeMillis,
        basis: EventTimeBasis,
    ) -> Result<SemanticHistoryOutcome, SemanticHistoryError<B::Error, D::Error, L::Error>> {
        let effective_at = match basis {
            EventTimeBasis::DeviceRecorded => {
                let Some(device_time) = observation.device_recorded_at() else {
                    return Err(SemanticHistoryError::MissingDeviceRecordedTime {
                        observation: Box::new(observation),
                        known_at,
                        basis,
                    });
                };
                device_time
            }
            EventTimeBasis::ReceivedAt => observation.received_at(),
        };
        let retry_observation = observation.clone();
        let context = ResolutionContext::new(effective_at, known_at);
        let outcome = self
            .resolver
            .resolve(observation, context)
            .await
            .map_err(|source| SemanticHistoryError::Resolution {
                source,
                observation: Box::new(retry_observation.clone()),
                known_at,
                basis,
            })?;
        let bound = match outcome {
            ResolutionOutcome::Bound(bound) => bound,
            ResolutionOutcome::Unresolved(unresolved) => {
                return Ok(SemanticHistoryOutcome::Unresolved(unresolved));
            }
        };
        let change = AssetPropertyChange::new(bound, basis).map_err(|source| {
            SemanticHistoryError::ChangeConstruction {
                source,
                observation: Box::new(retry_observation),
                known_at,
                basis,
            }
        })?;
        self.append_change(change).await
    }

    /// Retries one exact semantic change after an ambiguous append failure.
    ///
    /// This bypasses re-resolution, which could select a later binding
    /// correction and therefore produce another interpretation identity.
    ///
    /// # Errors
    ///
    /// Returns the append error together with the unchanged candidate.
    pub async fn append_change(
        &self,
        change: AssetPropertyChange,
    ) -> Result<SemanticHistoryOutcome, SemanticHistoryError<B::Error, D::Error, L::Error>> {
        match self.log.append(&change).await {
            Ok(append) => Ok(SemanticHistoryOutcome::Recorded { change, append }),
            Err(source) => Err(SemanticHistoryError::Append {
                source,
                change: Box::new(change),
            }),
        }
    }
}

/// Result of processing one decoded observation.
#[derive(Clone, Eq, PartialEq)]
pub enum SemanticHistoryOutcome {
    /// A compatible interpretation is durable, including exact retries.
    Recorded {
        /// The complete semantic change and provenance.
        change: AssetPropertyChange,
        /// Whether this call inserted it or found it already durable.
        append: PropertyChangeAppendOutcome,
    },
    /// Configuration or compatibility requires later operator action.
    Unresolved(UnresolvedObservation),
}

/// Exact input that the caller retains after a processing error.
pub enum SemanticHistoryRetry {
    /// Resolve the original observation under the same time policy after recovery.
    Observation {
        /// Original source fact.
        observation: Box<DecodedObservation>,
        /// Inclusive recorded-time cutoff selected by the caller.
        known_at: UnixTimeMillis,
        /// Timestamp choice selected by the caller.
        basis: EventTimeBasis,
    },
    /// Retry the same change without selecting another binding revision.
    Change(Box<AssetPropertyChange>),
}

/// Failure while resolving or appending one semantic interpretation.
pub enum SemanticHistoryError<B, D, L> {
    /// The selected device timestamp is absent; no read or write occurred.
    MissingDeviceRecordedTime {
        /// Original decoded observation available for another policy choice.
        observation: Box<DecodedObservation>,
        /// Original inclusive knowledge cutoff.
        known_at: UnixTimeMillis,
        /// Originally selected timestamp basis.
        basis: EventTimeBasis,
    },
    /// Semantic read policy failed before a change was formed.
    Resolution {
        /// Exact read-side cause.
        source: ResolutionError<B, D>,
        /// Original observation available for later re-resolution.
        observation: Box<DecodedObservation>,
        /// Original inclusive knowledge cutoff.
        known_at: UnixTimeMillis,
        /// Originally selected timestamp basis.
        basis: EventTimeBasis,
    },
    /// Event-time construction failed after a binding resolved.
    ChangeConstruction {
        /// Exact value-construction cause.
        source: AssetPropertyChangeError,
        /// Original observation available for later re-resolution.
        observation: Box<DecodedObservation>,
        /// Original inclusive knowledge cutoff.
        known_at: UnixTimeMillis,
        /// Originally selected timestamp basis.
        basis: EventTimeBasis,
    },
    /// Durable append failed or its outcome was ambiguous.
    Append {
        /// Exact append-side cause.
        source: L,
        /// The exact interpretation, retained independently of later bindings.
        change: Box<AssetPropertyChange>,
    },
}

impl<B, D, L> SemanticHistoryError<B, D, L> {
    /// Recovers the exact input needed for a safe retry.
    pub fn into_retry(self) -> SemanticHistoryRetry {
        match self {
            Self::MissingDeviceRecordedTime {
                observation,
                known_at,
                basis,
            }
            | Self::Resolution {
                observation,
                known_at,
                basis,
                ..
            }
            | Self::ChangeConstruction {
                observation,
                known_at,
                basis,
                ..
            } => SemanticHistoryRetry::Observation {
                observation,
                known_at,
                basis,
            },
            Self::Append { change, .. } => SemanticHistoryRetry::Change(change),
        }
    }
}

impl<B, D, L> fmt::Debug for SemanticHistoryError<B, D, L> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::MissingDeviceRecordedTime { .. } => "missing device time",
            Self::Resolution { .. } => "resolution failure",
            Self::ChangeConstruction { .. } => "change construction failure",
            Self::Append { .. } => "history append failure",
        };
        formatter
            .debug_struct("SemanticHistoryError")
            .field("kind", &kind)
            .finish_non_exhaustive()
    }
}

impl<B, D, L> fmt::Display for SemanticHistoryError<B, D, L> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDeviceRecordedTime { .. } => {
                formatter.write_str("selected device time is absent")
            }
            Self::Resolution { .. } => formatter.write_str("semantic resolution failed"),
            Self::ChangeConstruction { .. } => {
                formatter.write_str("semantic change construction failed")
            }
            Self::Append { .. } => formatter.write_str("semantic history append failed"),
        }
    }
}

impl<B, D, L> Error for SemanticHistoryError<B, D, L>
where
    B: Error + 'static,
    D: Error + 'static,
    L: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::MissingDeviceRecordedTime { .. } => None,
            Self::Resolution { source, .. } => Some(source),
            Self::ChangeConstruction { source, .. } => Some(source),
            Self::Append { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests;

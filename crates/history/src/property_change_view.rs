//! Read contracts for immutable semantic property interpretations.

use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{SignalTarget, TenantId, UnixTimeMillis};
use fleetiq_semantics::{AssetPropertyChangeId, EventTimeBasis, ResolutionContext};
use fleetiq_telemetry::DecodedObservation;

use crate::{ObservationInterpretationPage, ObservationInterpretationPageQuery};

/// One stored interpretation with its immutable source and target provenance.
///
/// The resolution context is evidence from the first successful append. It is
/// not an as-of query cutoff or a claim that this interpretation is current.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredPropertyChange {
    id: AssetPropertyChangeId,
    observation: DecodedObservation,
    target: SignalTarget,
    event_at: UnixTimeMillis,
    first_resolution: ResolutionContext,
}

impl StoredPropertyChange {
    /// Reconstructs a stored interpretation from validated source and target values.
    ///
    /// # Errors
    ///
    /// Rejects a source identity or selected event time that disagrees with the
    /// requested immutable interpretation identity.
    pub fn new(
        id: AssetPropertyChangeId,
        observation: DecodedObservation,
        target: SignalTarget,
        event_at: UnixTimeMillis,
        first_resolution: ResolutionContext,
    ) -> Result<Self, InvalidStoredPropertyChange> {
        if observation.id() != id.observation_id() {
            return Err(InvalidStoredPropertyChange::ObservationIdentity);
        }
        let expected_time = match id.event_time_basis() {
            EventTimeBasis::DeviceRecorded => observation
                .device_recorded_at()
                .ok_or(InvalidStoredPropertyChange::EventTime)?,
            EventTimeBasis::ReceivedAt => observation.received_at(),
        };
        if event_at != expected_time {
            return Err(InvalidStoredPropertyChange::EventTime);
        }
        Ok(Self {
            id,
            observation,
            target,
            event_at,
            first_resolution,
        })
    }

    /// Returns the exact decoded-output, binding-revision, and time-basis key.
    pub const fn id(&self) -> &AssetPropertyChangeId {
        &self.id
    }

    /// Returns the tenant established when the source message was accepted.
    pub const fn tenant_id(&self) -> &TenantId {
        self.observation.tenant_id()
    }

    /// Returns the durable decoded source, including value, quality, and times.
    pub const fn observation(&self) -> &DecodedObservation {
        &self.observation
    }

    /// Returns the asset and exact property definition selected by the binding.
    pub const fn target(&self) -> &SignalTarget {
        &self.target
    }

    /// Returns the selected event time, validated against the source fact.
    pub const fn event_at(&self) -> UnixTimeMillis {
        self.event_at
    }

    /// Returns the cutoffs recorded on the first successful append.
    pub const fn first_resolution(&self) -> ResolutionContext {
        self.first_resolution
    }
}

/// Persisted interpretation content disagrees with its immutable identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidStoredPropertyChange {
    /// The loaded decoded output has another identity.
    ObservationIdentity,
    /// The stored event time differs from the selected source timestamp.
    EventTime,
}

impl fmt::Display for InvalidStoredPropertyChange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ObservationIdentity => formatter.write_str("stored observation identity differs"),
            Self::EventTime => formatter.write_str("stored event time differs from source fact"),
        }
    }
}

impl Error for InvalidStoredPropertyChange {}

/// Reads exact or source-scoped tenant-owned interpretations without selecting a latest value.
///
/// Absence is distinct from repository failure. Both binding corrections and
/// different event-time policies have different identities and remain readable.
/// The implementation must not fall back to another tenant, output, binding
/// revision, or time basis.
pub trait AssetPropertyChangeView: Send + Sync {
    /// Infrastructure-specific failure.
    type Error: Error + Send + Sync + 'static;

    /// Finds the immutable interpretation and its source/target provenance.
    fn find<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        id: &'a AssetPropertyChangeId,
    ) -> impl Future<Output = Result<Option<StoredPropertyChange>, Self::Error>> + Send + 'a;
    /// Lists one bounded page of this source output's interpretations.
    ///
    /// The implementation must read rows set-wise. It must not invoke the
    /// exact lookup once per result or choose a latest binding revision.
    fn list_for_observation<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        query: &'a ObservationInterpretationPageQuery,
    ) -> impl Future<Output = Result<ObservationInterpretationPage, Self::Error>> + Send + 'a;
}

#[cfg(test)]
mod tests;

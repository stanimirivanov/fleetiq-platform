use std::{error::Error, fmt};

use fleetiq_domain::{
    RecordRevision, SignalBindingId, SignalTarget, TenantId, UnitId, UnixTimeMillis,
};
use fleetiq_telemetry::{DecodedObservationId, DecodedValue, ObservationQuality};

use crate::BoundObservation;

/// Identifies which preserved timestamp was selected as event time.
///
/// Selection is an explicit caller policy. The change contract does not infer
/// whether a device clock is trustworthy or whether receipt time is a suitable
/// fallback.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EventTimeBasis {
    /// Use the timestamp reported by the device.
    DeviceRecorded,
    /// Use the time at which the host received the complete framed message.
    ReceivedAt,
}

/// Deterministically identifies one semantic interpretation of an observation.
///
/// Binding provenance distinguishes a corrected attribution from the
/// interpretation produced by an earlier binding revision. The event-time
/// basis distinguishes policy choices that place the same observation at a
/// different event-time instant.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AssetPropertyChangeId {
    observation_id: DecodedObservationId,
    binding_id: SignalBindingId,
    binding_revision: RecordRevision,
    event_time_basis: EventTimeBasis,
}

impl AssetPropertyChangeId {
    fn new(bound_observation: &BoundObservation, event_time_basis: EventTimeBasis) -> Self {
        Self {
            observation_id: bound_observation.id().clone(),
            binding_id: bound_observation.binding().binding_id().clone(),
            binding_revision: bound_observation.binding().revision(),
            event_time_basis,
        }
    }

    /// Returns the immutable decoded observation being interpreted.
    pub const fn observation_id(&self) -> &DecodedObservationId {
        &self.observation_id
    }

    /// Returns the stable signal-binding identity used for attribution.
    pub const fn binding_id(&self) -> &SignalBindingId {
        &self.binding_id
    }

    /// Returns the exact signal-binding revision used for attribution.
    pub const fn binding_revision(&self) -> RecordRevision {
        self.binding_revision
    }

    /// Returns the timestamp basis used by this interpretation.
    pub const fn event_time_basis(&self) -> EventTimeBasis {
        self.event_time_basis
    }
}

/// One semantically attributed asset-property update candidate.
///
/// The change preserves the complete bound observation and makes its selected
/// event time explicit. It represents an observed value for downstream history,
/// state projection, and calculation; it does not assert that the value differs
/// from a previously stored value or that it is eligible to replace latest
/// state. Source authority, quality acceptance, ordering, and freshness remain
/// downstream policies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetPropertyChange {
    id: AssetPropertyChangeId,
    bound_observation: BoundObservation,
    event_at: UnixTimeMillis,
    event_time_basis: EventTimeBasis,
}

impl AssetPropertyChange {
    /// Creates an asset-property change using one preserved timestamp.
    ///
    /// # Errors
    ///
    /// Returns [`AssetPropertyChangeError::MissingDeviceRecordedTime`] when
    /// [`EventTimeBasis::DeviceRecorded`] is requested for an observation that
    /// does not contain a device timestamp.
    pub fn new(
        bound_observation: BoundObservation,
        event_time_basis: EventTimeBasis,
    ) -> Result<Self, AssetPropertyChangeError> {
        let event_at = match event_time_basis {
            EventTimeBasis::DeviceRecorded => bound_observation
                .observation()
                .device_recorded_at()
                .ok_or(AssetPropertyChangeError::MissingDeviceRecordedTime)?,
            EventTimeBasis::ReceivedAt => bound_observation.observation().received_at(),
        };

        let id = AssetPropertyChangeId::new(&bound_observation, event_time_basis);

        Ok(Self {
            id,
            bound_observation,
            event_at,
            event_time_basis,
        })
    }

    /// Returns this semantic interpretation's deterministic identity.
    pub const fn id(&self) -> &AssetPropertyChangeId {
        &self.id
    }

    /// Returns the tenant established when the source message was admitted.
    pub const fn tenant_id(&self) -> &TenantId {
        self.bound_observation.observation().tenant_id()
    }

    /// Returns the asset and exact property definition receiving the value.
    pub const fn target(&self) -> &SignalTarget {
        self.bound_observation.target()
    }

    /// Returns the semantically compatible value.
    pub const fn value(&self) -> &DecodedValue {
        self.bound_observation.observation().value()
    }

    /// Returns the canonical unit verified during semantic binding, if any.
    pub const fn unit(&self) -> Option<&UnitId> {
        self.bound_observation.observation().source_unit()
    }

    /// Returns the source-reported quality retained by semantic binding.
    pub const fn quality(&self) -> ObservationQuality {
        self.bound_observation.observation().quality()
    }

    /// Returns the timestamp selected for event-time processing.
    pub const fn event_at(&self) -> UnixTimeMillis {
        self.event_at
    }

    /// Returns which preserved timestamp supplied the event time.
    pub const fn event_time_basis(&self) -> EventTimeBasis {
        self.event_time_basis
    }

    /// Returns the bound observation with complete source and binding lineage.
    pub const fn bound_observation(&self) -> &BoundObservation {
        &self.bound_observation
    }

    /// Consumes the change and returns its bound observation.
    pub fn into_bound_observation(self) -> BoundObservation {
        self.bound_observation
    }
}

/// Describes why an asset-property change cannot use the requested event time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetPropertyChangeError {
    /// Device time was selected but the decoded observation did not contain it.
    MissingDeviceRecordedTime,
}

impl fmt::Display for AssetPropertyChangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDeviceRecordedTime => {
                formatter.write_str("device-recorded event time is unavailable")
            }
        }
    }
}

impl Error for AssetPropertyChangeError {}

#[cfg(test)]
mod tests;

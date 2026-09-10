//! Protocol-neutral decoded observation contracts.

mod measurement;
mod observation;
mod provenance;

pub use measurement::{
    DecodedMeasurement, DecodedMeasurementError, DecodedValue, ObservationQuality,
};
pub use observation::{DecodedObservation, ObservationTimes};
pub use provenance::{DecodedObservationId, ProtocolPackageRef};

#[cfg(test)]
mod tests;

//! Conservative point attribution: no pair interpolation or coordinate guessing.
use super::{CoordinateEncoding, FleetAsset, LocationMapping};
use crate::{Freshness, freshness};
use fleetiq_domain::{PropertyValue, UnitId, UnixTimeMillis};
use fleetiq_semantics::EventTimeBasis;
use fleetiq_telemetry::ObservationQuality;
/// A usable coherent point; exact decimal strings precede any renderer conversion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedLocation {
    /// Exact degrees in [-90, 90].
    pub latitude: String,
    /// Exact degrees in [-180, 180].
    pub longitude: String,
    /// Conservative combined source quality; invalid candidates never produce a point.
    pub quality: ObservationQuality,
    /// Age at the response evaluation instant, independent of quality.
    pub freshness: Freshness,
}
/// Missing/invalid coordinate evidence is not an asset-health state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocationState {
    /// Installed type does not declare both mapped definitions.
    Unmapped,
    /// Definition kind/unit is incompatible with the explicitly chosen encoding.
    IncompatibleMapping,
    /// At least one current coordinate is absent.
    Missing,
    /// Source identity, device time or source uniqueness cannot establish a pair.
    Incoherent,
    /// Coordinates exceed geographic bounds or 128-character exact output budget.
    InvalidCoordinates,
    /// A current coordinate is explicitly invalid; do not select an older value.
    InvalidQuality,
    /// A coherent point retaining independent quality and evaluation age.
    Observed(ObservedLocation),
}
/// Classifies current candidates without converting through floating point.
#[must_use]
pub fn classify_location(
    asset: &FleetAsset,
    mapping: &LocationMapping,
    supported: bool,
    at: UnixTimeMillis,
) -> LocationState {
    if !asset.mapped {
        return LocationState::Unmapped;
    }
    if !supported {
        return LocationState::IncompatibleMapping;
    }
    let (Some(lat), Some(lon)) = (&asset.latitude, &asset.longitude) else {
        return LocationState::Missing;
    };
    let a = lat.observation();
    let b = lon.observation();
    if asset.ambiguous
        || lat.tenant_id() != lon.tenant_id()
        || lat.target().asset_id() != &asset.id
        || lon.target().asset_id() != &asset.id
        || lat.target().property() != mapping.latitude()
        || lon.target().property() != mapping.longitude()
        || a.source().device_asset_id() != b.source().device_asset_id()
        || a.source().endpoint_id() != b.source().endpoint_id()
        || a.id().message_id() != b.id().message_id()
        || a.decoded_by() != b.decoded_by()
        || a.id().output_index() == b.id().output_index()
        || a.source().signal_id() == b.source().signal_id()
        || a.device_recorded_at().is_none()
        || a.device_recorded_at() != b.device_recorded_at()
        || lat.id().event_time_basis() != EventTimeBasis::DeviceRecorded
        || lon.id().event_time_basis() != EventTimeBasis::DeviceRecorded
        || lat.event_at() != lon.event_at()
    {
        return LocationState::Incoherent;
    }
    if a.quality() == ObservationQuality::Invalid || b.quality() == ObservationQuality::Invalid {
        return LocationState::InvalidQuality;
    }
    let unit = match mapping.encoding() {
        CoordinateEncoding::Degrees => Some("deg"),
        CoordinateEncoding::E7 => None,
    };
    if a.source_unit().map(UnitId::as_str) != unit || b.source_unit().map(UnitId::as_str) != unit {
        return LocationState::InvalidCoordinates;
    }
    let (Some(latitude), Some(longitude)) = (
        coordinate(a.value(), mapping.encoding(), 90),
        coordinate(b.value(), mapping.encoding(), 180),
    ) else {
        return LocationState::InvalidCoordinates;
    };
    LocationState::Observed(ObservedLocation {
        latitude,
        longitude,
        quality: if a.quality() == ObservationQuality::Uncertain
            || b.quality() == ObservationQuality::Uncertain
        {
            ObservationQuality::Uncertain
        } else {
            ObservationQuality::Good
        },
        freshness: freshness(lat.event_at(), at),
    })
}
fn coordinate(value: &PropertyValue, encoding: CoordinateEncoding, maximum: i32) -> Option<String> {
    let result = match (encoding, value) {
        (CoordinateEncoding::E7, PropertyValue::Integer(n)) => {
            if !(-i64::from(maximum) * 10_000_000..=i64::from(maximum) * 10_000_000).contains(n) {
                return None;
            }
            format!(
                "{}{}.{:07}",
                if *n < 0 { "-" } else { "" },
                n.abs() / 10_000_000,
                n.abs() % 10_000_000
            )
        }
        (CoordinateEncoding::Degrees, PropertyValue::Integer(n))
            if (-i64::from(maximum)..=i64::from(maximum)).contains(n) =>
        {
            n.to_string()
        }
        (CoordinateEncoding::Degrees, PropertyValue::Decimal(n))
            if n >= -maximum && n <= maximum =>
        {
            n.normalized().to_string()
        }
        _ => return None,
    };
    (result.len() <= 128).then_some(result)
}
#[cfg(test)]
mod tests;

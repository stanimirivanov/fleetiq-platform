//! Version-one flat string columns; never narrow exact numeric evidence.
use crate::{ArchiveError, MAX_FIELD_BYTES};
use fleetiq_domain::{
    AssetId, DefinitionVersion, DeviceEndpointId, PropertyDefinitionId, PropertyDefinitionRef,
    PropertyValue, RecordRevision, SignalBindingId, SignalSource, SignalTarget, SourceSignalId,
    TenantId, UnitId, UnixTimeMillis,
};
use fleetiq_history::StoredPropertyChange;
use fleetiq_semantics::{AssetPropertyChangeId, EventTimeBasis, ResolutionContext};
use fleetiq_telemetry::{
    AcceptedMessageId, DecodedMeasurement, DecodedObservation, DecodedObservationId,
    ObservationQuality, ObservationTimes, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};
pub(crate) const NAMES: [&str; 23] = [
    "tenant",
    "message",
    "package",
    "package_version",
    "output",
    "binding",
    "revision",
    "basis",
    "event",
    "effective",
    "known",
    "target",
    "property",
    "property_version",
    "device",
    "endpoint",
    "signal",
    "device_at",
    "received",
    "value_kind",
    "value",
    "unit",
    "quality",
];
pub(crate) type Fields = [String; 23];
fn invalid<T, E>(r: Result<T, E>) -> Result<T, ArchiveError> {
    r.map_err(|_| ArchiveError)
}
pub(crate) fn fields(c: &StoredPropertyChange) -> Result<Fields, ArchiveError> {
    let s = c.observation();
    let id = c.id();
    let source = s.source();
    let (kind, value) = match s.value() {
        PropertyValue::Boolean(v) => ("boolean", v.to_string()),
        PropertyValue::Integer(v) => ("integer", v.to_string()),
        PropertyValue::Decimal(v) => {
            if v.digits() > MAX_FIELD_BYTES as u64
                || v.fractional_digit_count().unsigned_abs() > MAX_FIELD_BYTES as u64
            {
                return Err(ArchiveError);
            }
            ("decimal", v.to_string())
        }
        PropertyValue::Text(v) => {
            if v.len() > MAX_FIELD_BYTES {
                return Err(ArchiveError);
            }
            ("text", v.clone())
        }
    };
    Ok([
        c.tenant_id().as_str().into(),
        s.id().message_id().as_str().into(),
        s.decoded_by().id().as_str().into(),
        s.decoded_by().version().as_str().into(),
        s.id().output_index().to_string(),
        id.binding_id().as_str().into(),
        id.binding_revision().get().to_string(),
        match id.event_time_basis() {
            EventTimeBasis::DeviceRecorded => "device",
            EventTimeBasis::ReceivedAt => "received",
        }
        .into(),
        c.event_at().get().to_string(),
        c.first_resolution().effective_at().get().to_string(),
        c.first_resolution().known_at().get().to_string(),
        c.target().asset_id().as_str().into(),
        c.target().property().id().as_str().into(),
        c.target().property().version().get().to_string(),
        source.device_asset_id().as_str().into(),
        source.endpoint_id().as_str().into(),
        source.signal_id().as_str().into(),
        s.device_recorded_at()
            .map_or_else(String::new, |t| t.get().to_string()),
        s.received_at().get().to_string(),
        kind.into(),
        value,
        s.source_unit()
            .map_or_else(String::new, |u| u.as_str().into()),
        match s.quality() {
            ObservationQuality::Good => "good",
            ObservationQuality::Uncertain => "uncertain",
            ObservationQuality::Invalid => "invalid",
        }
        .into(),
    ])
}
pub(crate) fn reconstruct(f: &Fields) -> Result<StoredPropertyChange, ArchiveError> {
    let tenant = invalid(TenantId::new(&f[0]))?;
    let package = ProtocolPackageRef::new(
        invalid(ProtocolPackageId::new(&f[2]))?,
        invalid(ProtocolPackageVersion::new(&f[3]))?,
    );
    let observation = DecodedObservationId::new(
        invalid(AcceptedMessageId::new(&f[1]))?,
        package,
        invalid(f[4].parse())?,
    );
    let value = match f[19].as_str() {
        "boolean" => PropertyValue::Boolean(invalid(f[20].parse())?),
        "integer" => PropertyValue::Integer(invalid(f[20].parse())?),
        "decimal" => {
            // Reject huge exponents before BigDecimal can be formatted later.
            let value = PropertyValue::Decimal(invalid(f[20].parse())?);
            if let PropertyValue::Decimal(v) = &value
                && (v.digits() > MAX_FIELD_BYTES as u64
                    || v.fractional_digit_count().unsigned_abs() > MAX_FIELD_BYTES as u64)
            {
                return Err(ArchiveError);
            }
            value
        }
        "text" => PropertyValue::Text(f[20].clone()),
        _ => return Err(ArchiveError),
    };
    let unit = if f[21].is_empty() {
        None
    } else {
        Some(invalid(UnitId::new(&f[21]))?)
    };
    let quality = match f[22].as_str() {
        "good" => ObservationQuality::Good,
        "uncertain" => ObservationQuality::Uncertain,
        "invalid" => ObservationQuality::Invalid,
        _ => return Err(ArchiveError),
    };
    let measurement = invalid(DecodedMeasurement::new(value, unit, quality))?;
    let source = SignalSource::new(
        invalid(AssetId::new(&f[14]))?,
        invalid(DeviceEndpointId::new(&f[15]))?,
        invalid(SourceSignalId::new(&f[16]))?,
    );
    let device = if f[17].is_empty() {
        None
    } else {
        Some(UnixTimeMillis::new(invalid(f[17].parse())?))
    };
    let source = DecodedObservation::new(
        observation.clone(),
        tenant,
        source,
        measurement,
        ObservationTimes::new(device, UnixTimeMillis::new(invalid(f[18].parse())?)),
    );
    let basis = match f[7].as_str() {
        "device" => EventTimeBasis::DeviceRecorded,
        "received" => EventTimeBasis::ReceivedAt,
        _ => return Err(ArchiveError),
    };
    let id = AssetPropertyChangeId::from_parts(
        observation,
        invalid(SignalBindingId::new(&f[5]))?,
        invalid(RecordRevision::new(invalid(f[6].parse())?))?,
        basis,
    );
    let target = SignalTarget::new(
        invalid(AssetId::new(&f[11]))?,
        PropertyDefinitionRef::new(
            invalid(PropertyDefinitionId::new(&f[12]))?,
            invalid(DefinitionVersion::new(invalid(f[13].parse())?))?,
        ),
    );
    let record = invalid(StoredPropertyChange::new(
        id,
        source,
        target,
        UnixTimeMillis::new(invalid(f[8].parse())?),
        ResolutionContext::new(
            UnixTimeMillis::new(invalid(f[9].parse())?),
            UnixTimeMillis::new(invalid(f[10].parse())?),
        ),
    ))?;
    // Canonical forms prevent alternative spellings from bypassing identity checks.
    if fields(&record)? != *f {
        return Err(ArchiveError);
    }
    Ok(record)
}

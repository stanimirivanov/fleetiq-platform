//! Exact evidence and hostile segment regression fixtures.
use fleetiq_archive::{decode, encode};
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
use parquet::{
    file::{reader::FileReader, serialized_reader::SerializedFileReader},
    record::RowAccessor,
};
fn evidence(index: u32, value: PropertyValue) -> StoredPropertyChange {
    let unit = value.is_numeric().then(|| UnitId::new("volt").unwrap());
    let quality = match index % 3 {
        0 => ObservationQuality::Good,
        1 => ObservationQuality::Uncertain,
        _ => ObservationQuality::Invalid,
    };
    let device = (index.is_multiple_of(2)).then_some(UnixTimeMillis::new(i64::MIN));
    let basis = if device.is_some() {
        EventTimeBasis::DeviceRecorded
    } else {
        EventTimeBasis::ReceivedAt
    };
    let event = device.unwrap_or(UnixTimeMillis::new(i64::MAX));
    let source = DecodedObservation::new(
        DecodedObservationId::new(
            AcceptedMessageId::new(format!("message-{index}")).unwrap(),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("vendor.binary").unwrap(),
                ProtocolPackageVersion::new("2026.11").unwrap(),
            ),
            index,
        ),
        TenantId::new("tenant-a").unwrap(),
        SignalSource::new(
            AssetId::new("device").unwrap(),
            DeviceEndpointId::new("can").unwrap(),
            SourceSignalId::new("value").unwrap(),
        ),
        DecodedMeasurement::new(value, unit, quality).unwrap(),
        ObservationTimes::new(device, UnixTimeMillis::new(i64::MAX)),
    );
    StoredPropertyChange::new(
        AssetPropertyChangeId::from_parts(
            source.id().clone(),
            SignalBindingId::new("binding").unwrap(),
            RecordRevision::new(u64::MAX).unwrap(),
            basis,
        ),
        source,
        SignalTarget::new(
            AssetId::new("target").unwrap(),
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("value").unwrap(),
                DefinitionVersion::new(u32::MAX).unwrap(),
            ),
        ),
        event,
        ResolutionContext::new(UnixTimeMillis::new(i64::MAX), UnixTimeMillis::new(-1)),
    )
    .unwrap()
}
#[test]
fn exact_evidence_round_trips_and_rejects_bad_segments() {
    let tenant = TenantId::new("tenant-a").unwrap();
    let rows = vec![
        evidence(0, PropertyValue::Integer(i64::MIN)),
        evidence(
            1,
            PropertyValue::Decimal(
                "123456789012345678901234567890.000000000000000001"
                    .parse()
                    .unwrap(),
            ),
        ),
        evidence(2, PropertyValue::Boolean(true)),
        evidence(3, PropertyValue::Text("\0\n日本語".into())),
    ];
    let bytes = encode(&tenant, &rows).unwrap();
    let native = SerializedFileReader::new(bytes::Bytes::copy_from_slice(&bytes)).unwrap();
    let first = native.get_row_iter(None).unwrap().next().unwrap().unwrap();
    assert_eq!(first.get_string(0).unwrap(), "tenant-a");
    assert_eq!(first.get_string(20).unwrap(), &i64::MIN.to_string());
    for position in (0..bytes.len()).step_by(31) {
        let mut mutated = bytes.clone();
        mutated[position] ^= 128;
        if let Ok(decoded) = decode(&tenant, &mutated) {
            assert_eq!(decoded, rows);
        }
    }
    assert_eq!(decode(&tenant, &bytes).unwrap(), rows);
    assert!(decode(&TenantId::new("tenant-b").unwrap(), &bytes).is_err());
    assert!(decode(&tenant, &bytes[..bytes.len() - 1]).is_err());
    assert!(encode(&tenant, &[rows[0].clone(), rows[0].clone()]).is_err());
    assert!(encode(&TenantId::new("tenant-b").unwrap(), &rows).is_err());
}
#[test]
fn profile_limits_and_integrity_fail_closed() {
    use fleetiq_archive::{MAX_FIELD_BYTES, MAX_FILE_BYTES, MAX_ROWS};
    let tenant = TenantId::new("tenant-a").unwrap();
    assert!(encode(&tenant, &[]).is_err());
    let rows = (0..=MAX_ROWS)
        .map(|i| evidence(u32::try_from(i).unwrap(), PropertyValue::Integer(1)))
        .collect::<Vec<_>>();
    assert!(encode(&tenant, &rows).is_err());
    assert!(
        encode(
            &tenant,
            &[evidence(
                0,
                PropertyValue::Text("x".repeat(MAX_FIELD_BYTES + 1))
            )]
        )
        .is_err()
    );
    let maximum = evidence(0, PropertyValue::Text("x".repeat(MAX_FIELD_BYTES)));
    assert_eq!(
        decode(
            &tenant,
            &encode(&tenant, std::slice::from_ref(&maximum)).unwrap()
        )
        .unwrap(),
        vec![maximum]
    );
    assert!(decode(&tenant, &vec![0; MAX_FILE_BYTES + 1]).is_err());
    assert!(
        encode(
            &tenant,
            &[evidence(
                0,
                PropertyValue::Decimal("1e1000000000".parse().unwrap())
            )]
        )
        .is_err()
    );
    let rows = (0..MAX_ROWS)
        .map(|i| {
            evidence(
                u32::try_from(i).unwrap(),
                PropertyValue::Text("x".repeat(MAX_FIELD_BYTES)),
            )
        })
        .collect::<Vec<_>>();
    assert!(encode(&tenant, &rows).is_err());
    let bytes = encode(
        &tenant,
        &[evidence(
            0,
            PropertyValue::Text("archive-test-sentinel".into()),
        )],
    )
    .unwrap();
    for cut in 0..bytes.len() {
        assert!(decode(&tenant, &bytes[..cut]).is_err());
    }
    let mut unsupported = bytes.clone();
    let position = unsupported
        .windows(19)
        .position(|b| b == b"property-history-v1")
        .unwrap();
    unsupported[position + 18] = b'2';
    assert!(decode(&tenant, &unsupported).is_err());
    let mut damaged = bytes.clone();
    let position = damaged
        .windows(21)
        .position(|b| b == b"archive-test-sentinel")
        .unwrap();
    damaged[position] = b'X';
    assert!(decode(&tenant, &damaged).is_err());
    let mut footer = bytes.clone();
    let end = footer.len();
    footer[end - 8..end - 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode(&tenant, &footer).is_err());
    // Native page decoder is never reached for V2, dictionary or oversized headers.
    for prefix in [1, 2, 4, 255] {
        let mut page = bytes.clone();
        page[5] = prefix;
        assert!(decode(&tenant, &page).is_err());
    }
}
#[test]
fn representative_segment() {
    let tenant = TenantId::new("tenant-a").unwrap();
    let rows = (0..fleetiq_archive::MAX_ROWS)
        .map(|i| {
            evidence(
                u32::try_from(i).unwrap(),
                PropertyValue::Text("synthetic-reading-".repeat(170)),
            )
        })
        .collect::<Vec<_>>();
    let bytes = encode(&tenant, &rows).unwrap();
    assert_eq!(decode(&tenant, &bytes).unwrap(), rows);
    println!(
        "archive_profile=v1 rows={} encoded_bytes={} file_cap={}",
        rows.len(),
        bytes.len(),
        fleetiq_archive::MAX_FILE_BYTES
    );
}

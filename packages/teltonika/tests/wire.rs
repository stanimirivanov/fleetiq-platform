//! Synthetic fixtures exercise wire invariants independently of the component.
use fleetiq_teltonika::{WireError, decode, identify, locate_frame};
fn hex(text: &str) -> Vec<u8> {
    text.trim()
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}
fn frames() -> [Vec<u8>; 2] {
    [
        hex(include_str!("fixtures/codec8.hex")),
        hex(include_str!("fixtures/codec8-extended.hex")),
    ]
}
// Test-side resealing allows malformed records to pass the independent CRC gate.
fn seal(frame: &mut [u8]) {
    let end = frame.len() - 4;
    let mut crc = 0_u16;
    for &b in &frame[8..end] {
        crc ^= u16::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xa001
            } else {
                crc >> 1
            };
        }
    }
    frame[end..].copy_from_slice(&u32::from(crc).to_be_bytes());
}
#[test]
fn both_codecs_preserve_time_negative_coordinates_and_unknown_io_bytes() {
    for frame in frames() {
        let packet = decode(&frame).unwrap();
        assert_eq!(packet.records.len(), 2);
        let record = &packet.records[0];
        assert_eq!(record.timestamp_ms, 1_700_000_000_123);
        assert_eq!(record.gps.longitude, -123_456_789);
        assert_eq!(record.gps.latitude, 523_456_789);
        assert_eq!(record.gps.altitude_raw, 65_530);
        assert_eq!(record.gps.speed, 42);
        assert_eq!(record.io[3].bytes, vec![255; 8]);
        assert_eq!(record.io[3].id, 253);
        assert_eq!(packet.records[1].gps.satellites, 0);
        if packet.codec == 0x8e {
            assert_eq!(record.io[4].id, 60_000);
            assert_eq!(record.io[4].bytes, [0, 171, 255]);
        }
        assert_eq!(decode(&frame).unwrap(), packet);
    }
}
#[test]
fn every_fragment_and_coalesced_prefix_has_one_frame_boundary() {
    let hello = b"\x00\x0f123456789012345";
    assert_eq!(identify(hello).unwrap(), "123456789012345");
    for n in 0..hello.len() {
        assert_eq!(locate_frame(&hello[..n], &[]), Ok(None));
    }
    for frame in frames() {
        for n in 0..frame.len() {
            assert_eq!(locate_frame(&frame[..n], &[1]), Ok(None));
            assert!(decode(&frame[..n]).is_err());
        }
        let joined = [frame.as_slice(), frame.as_slice()].concat();
        assert_eq!(locate_frame(&joined, &[1]), Ok(Some(frame.len())));
        assert!(decode(&joined).is_err());
        assert_eq!(
            locate_frame(&[hello.as_slice(), frame.as_slice()].concat(), &[]),
            Ok(Some(17))
        );
    }
}
#[test]
fn handshake_and_session_shapes_are_strict() {
    for bad in [
        b"".as_slice(),
        b"\x00\x0e12345678901234",
        b"\x00\x0f12345678901234!",
        b"\x00\x0f1234567890123450",
    ] {
        assert!(identify(bad).is_err());
    }
    assert!(locate_frame(&[1], &[]).is_err());
    assert!(locate_frame(&[], &[2]).is_err());
    assert!(locate_frame(b"\x00\x0f123456789012345", &[1]).is_err());
    assert!(locate_frame(&[0, 0, 0, 0, 255, 255, 255, 255], &[1]).is_err());
    assert!(locate_frame(&[0, 0, 0, 0, 0, 0, 0, 0], &[1]).is_err());
}
#[test]
fn crc_high_bits_payload_corruption_and_trailing_bytes_are_rejected() {
    for frame in frames() {
        for offset in [8, 15, frame.len() - 4, frame.len() - 1] {
            let mut bad = frame.clone();
            bad[offset] ^= 1;
            assert!(decode(&bad).is_err());
        }
        let mut bad = frame.clone();
        bad.push(0);
        assert!(decode(&bad).is_err());
        let mut bad = frame;
        bad[0] = 1;
        assert!(decode(&bad).is_err());
    }
}
#[test]
fn count_codec_gps_and_io_failures_survive_valid_crc() {
    let frame = frames()[0].clone();
    // Data begins at 8; record begins at 10; fixed GPS header is 24 bytes.
    for (offset, value) in [
        (8, 16),
        (9, 0),
        (9, 17),
        (18, 3),
        (35, 65),
        (36, 255),
        (36, 0),
        (37, 251),
    ] {
        let mut bad = frame.clone();
        bad[offset] = value;
        seal(&mut bad);
        assert!(decode(&bad).is_err(), "offset {offset}");
    }
    let mut bad = frame.clone();
    let end = bad.len() - 5;
    bad[end] = 1;
    seal(&mut bad);
    assert_eq!(decode(&bad), Err(WireError::Malformed));
    let mut bad = frame;
    bad[10..18].fill(255);
    seal(&mut bad);
    assert!(decode(&bad).is_err());
}
#[test]
fn duplicate_ids_and_extended_variable_bounds_are_rejected() {
    let mut frame = frames()[0].clone();
    frame[40] = 250;
    seal(&mut frame);
    assert_eq!(decode(&frame), Err(WireError::Malformed));
    // Extended first record: header24, event2,total2, groups (6+7+9+13), NX2, id2,length2.
    let frame = frames()[1].clone();
    for bytes in [[0, 0], [1, 1], [255, 255]] {
        let mut bad = frame.clone();
        bad[73..75].copy_from_slice(&bytes);
        seal(&mut bad);
        assert!(decode(&bad).is_err());
    }
}

#[test]
fn manufacturer_codec8_crc_vector_decodes_without_test_side_resealing() {
    // Teltonika Data Sending Protocols, Codec 8 example 1 (public synthetic frame).
    let packet = decode(&hex("000000000000003608010000016B40D8EA30010000000000000000000000000000000105021503010101425E0F01F10000601A014E0000000000000000010000C7CF")).unwrap();
    assert_eq!(packet.records[0].io.len(), 5);
    assert_eq!(packet.records[0].io[2].id, 66);
    assert_eq!(packet.records[0].io[2].bytes, [0x5e, 0x0f]);
}

#[test]
fn checksummed_body_truncation_never_returns_partial_records() {
    for frame in frames() {
        let end = frame.len() - 4;
        for cutoff in 11..end {
            let mut short = frame[..cutoff].to_vec();
            short.extend_from_slice(&[0; 4]);
            let length = u32::try_from(cutoff - 8).unwrap();
            short[4..8].copy_from_slice(&length.to_be_bytes());
            seal(&mut short);
            assert!(decode(&short).is_err(), "body truncated at {cutoff}");
        }
    }
}

fn packet_from_records(codec: u8, records: &[Vec<u8>]) -> Vec<u8> {
    let count = u8::try_from(records.len()).unwrap();
    let mut data = vec![codec, count];
    for record in records {
        data.extend_from_slice(record);
    }
    data.push(count);
    let mut frame = vec![0; 4];
    frame.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
    frame.extend_from_slice(&data);
    frame.extend_from_slice(&[0; 4]);
    seal(&mut frame);
    frame
}
#[test]
fn documented_record_io_and_variable_size_limits_are_accepted_at_the_boundary() {
    // Codec 8 minimum record: GPS/time header, event, total, four zero group counts.
    let empty_record = vec![0; 30];
    let packet = packet_from_records(8, &vec![empty_record; 16]);
    assert_eq!(decode(&packet).unwrap().records.len(), 16);
    let mut record = vec![0; 24];
    record.extend_from_slice(&[0, 64, 64]);
    for id in 0..64 {
        record.extend_from_slice(&[id, id]);
    }
    record.extend_from_slice(&[0; 3]);
    assert_eq!(
        decode(&packet_from_records(8, &[record])).unwrap().records[0]
            .io
            .len(),
        64
    );
    let mut record = vec![0; 24];
    record.extend_from_slice(&[0, 0, 0, 1]);
    record.extend_from_slice(&[0; 8]);
    record.extend_from_slice(&[0, 1, 0xea, 0x60, 1, 0]);
    record.extend_from_slice(&[0xa5; 256]);
    assert_eq!(
        decode(&packet_from_records(0x8e, &[record]))
            .unwrap()
            .records[0]
            .io[0]
            .bytes,
        vec![0xa5; 256]
    );
}

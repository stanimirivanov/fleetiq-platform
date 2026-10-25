//! Stable source signal vocabulary; no tenant, internal device or asset IDs.
use super::bindings::exports::fleetiq::protocol_package::runtime::{
    DecodedOutput, DecodedValue, ObservationQuality,
};
use crate::Packet;
use std::fmt::Write;
pub(super) fn outputs(packet: Packet) -> Vec<DecodedOutput> {
    let mut outputs = Vec::new();
    for record in packet.records {
        let time = Some(record.timestamp_ms);
        let good = record.gps.satellites != 0;
        let mut push = |signal: &str, value, unit: Option<&str>, gps: bool| {
            outputs.push(DecodedOutput {
                endpoint_id: "main".into(),
                signal_id: signal.into(),
                value,
                source_unit: unit.map(str::to_owned),
                device_recorded_at_ms: time,
                quality: if gps && !good {
                    ObservationQuality::Uncertain
                } else {
                    ObservationQuality::Good
                },
            });
        };
        push(
            "avl.codec",
            DecodedValue::Integer(i64::from(packet.codec)),
            None,
            false,
        );
        push(
            "avl.priority",
            DecodedValue::Integer(i64::from(record.priority)),
            None,
            false,
        );
        push(
            "avl.event_io_id",
            DecodedValue::Integer(i64::from(record.event_io_id)),
            None,
            false,
        );
        // Keep coordinate scaling in the signal name; avoid rounding or invented units.
        push(
            "gps.longitude_e7",
            DecodedValue::Integer(i64::from(record.gps.longitude)),
            None,
            true,
        );
        push(
            "gps.latitude_e7",
            DecodedValue::Integer(i64::from(record.gps.latitude)),
            None,
            true,
        );
        // The generic subset preserves altitude bits rather than imposing device signedness.
        push(
            "gps.altitude_raw",
            DecodedValue::Integer(i64::from(record.gps.altitude_raw)),
            None,
            true,
        );
        push(
            "gps.angle",
            DecodedValue::Integer(i64::from(record.gps.angle)),
            Some("deg"),
            true,
        );
        push(
            "gps.satellites",
            DecodedValue::Integer(i64::from(record.gps.satellites)),
            None,
            false,
        );
        push(
            "gps.speed",
            DecodedValue::Integer(i64::from(record.gps.speed)),
            Some("km/h"),
            true,
        );
        for io in record.io {
            let mut hex = String::with_capacity(io.bytes.len() * 2);
            for byte in io.bytes {
                let _ = write!(&mut hex, "{byte:02x}");
            }
            push(
                &format!("io.{}.raw", io.id),
                DecodedValue::Text(hex),
                None,
                false,
            );
        }
    }
    outputs
}

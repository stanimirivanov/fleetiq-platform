//! Versioned internal process envelope, separate from the guest WIT contract.
//! Parents bound encoded bytes before parsing. Every field is untrusted again
//! when reconstructed into native protocol values; no replies cross this boundary.
use crate::{
    DecodedMeasurement, DecodedOutput, DecodedValue, DeviceEndpointId, ObservationQuality,
    SourceSignalId, UnitId, UnixTimeMillis,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
/// Conservative execution identity: reviewed policy plus exact locked dependencies.
/// A changed lock requires draining with the prior immutable runtime.
#[must_use]
pub fn replay_fingerprint() -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"fleetiq-replay-policy-1\0");
    hash.update(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../Cargo.lock"
    )));
    hash.finalize().into()
}
/// Maximum JSON request bytes, including the eight-MiB artifact's numeric encoding.
pub const REPLAY_REQUEST_LIMIT: u64 = 48 * 1024 * 1024;
/// Maximum serialized execution result, before either endpoint parses it.
pub const REPLAY_RESPONSE_LIMIT: u64 = 2 * 1024 * 1024;
/// Version-one replay request; bytes travel only over private child stdin.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayRequest {
    /// Must equal 1; identifies fixed limits and envelope semantics.
    pub policy: u32,
    /// Exact host execution identity; incompatible workers must not execute it.
    pub execution: [u8; 32],
    /// Host-selected exact package identity.
    pub package_id: String,
    /// Host-selected immutable package version.
    pub package_version: String,
    /// runtime-v1 or identity-v1; must match durable admission evidence.
    pub world: String,
    /// Exact hash-verified component bytes.
    pub artifact: Vec<u8>,
    /// One complete frame.
    pub frame: Vec<u8>,
    /// Complete state before that frame.
    pub state: Vec<u8>,
}
/// Successful replay effects. No reply or socket identity can be encoded here.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayResponse {
    /// Exact replay policy used.
    pub policy: u32,
    /// Exact host execution identity; incompatible workers must not execute it.
    pub execution: [u8; 32],
    /// Complete successor checkpoint.
    pub state: Vec<u8>,
    /// Ordered source outputs, without host authority.
    pub outputs: Vec<ReplayOutput>,
}
/// Closed source-value wire alternatives; decimals retain exact base-ten text.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ReplayValue {
    /// Boolean source fact.
    Boolean(bool),
    /// Signed source integer.
    Integer(i64),
    /// Exact finite decimal text.
    Decimal(String),
    /// Uninterpreted source text.
    Text(String),
}
/// Native-adapter DTO; validating domain reconstruction remains mandatory.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayOutput {
    /// Source endpoint, never an asset identifier.
    pub endpoint: String,
    /// Source signal, never a property mapping.
    pub signal: String,
    /// Source value.
    pub value: ReplayValue,
    /// Optional protocol unit.
    pub unit: Option<String>,
    /// Optional device time in milliseconds.
    pub time: Option<i64>,
    /// Closed quality: good, uncertain, invalid.
    pub quality: String,
}
impl ReplayOutput {
    /// Copies one host-validated output into the internal process envelope.
    pub fn from_output(value: &DecodedOutput) -> Self {
        Self {
            endpoint: value.endpoint_id().as_str().into(),
            signal: value.signal_id().as_str().into(),
            value: match value.value() {
                DecodedValue::Boolean(v) => ReplayValue::Boolean(*v),
                DecodedValue::Integer(v) => ReplayValue::Integer(*v),
                DecodedValue::Decimal(v) => ReplayValue::Decimal(v.to_string()),
                DecodedValue::Text(v) => ReplayValue::Text(v.clone()),
            },
            unit: value.source_unit().map(|u| u.as_str().into()),
            time: value.device_recorded_at().map(UnixTimeMillis::get),
            quality: match value.quality() {
                ObservationQuality::Good => "good",
                ObservationQuality::Uncertain => "uncertain",
                ObservationQuality::Invalid => "invalid",
            }
            .into(),
        }
    }
    /// Reconstructs one validated native output; errors disclose no DTO content.
    /// # Errors
    /// Rejects invalid identifiers, values, units or quality alternatives.
    pub fn into_output(self) -> Result<DecodedOutput, InvalidReplayOutput> {
        match &self.value {
            ReplayValue::Decimal(v) if v.len() > 256 => return Err(InvalidReplayOutput),
            ReplayValue::Text(v) if v.len() > 8192 => return Err(InvalidReplayOutput),
            _ => {}
        }
        let value = match self.value {
            ReplayValue::Boolean(v) => DecodedValue::Boolean(v),
            ReplayValue::Integer(v) => DecodedValue::Integer(v),
            ReplayValue::Decimal(v) => {
                DecodedValue::Decimal(v.parse().map_err(|_| InvalidReplayOutput)?)
            }
            ReplayValue::Text(v) => DecodedValue::Text(v),
        };
        if let DecodedValue::Decimal(decimal) = &value
            && decimal.as_bigint_and_exponent().1.unsigned_abs() > 128
        {
            return Err(InvalidReplayOutput);
        }
        let quality = match self.quality.as_str() {
            "good" => ObservationQuality::Good,
            "uncertain" => ObservationQuality::Uncertain,
            "invalid" => ObservationQuality::Invalid,
            _ => return Err(InvalidReplayOutput),
        };
        let unit = self
            .unit
            .map(UnitId::new)
            .transpose()
            .map_err(|_| InvalidReplayOutput)?;
        let measurement =
            DecodedMeasurement::new(value, unit, quality).map_err(|_| InvalidReplayOutput)?;
        Ok(DecodedOutput::new(
            DeviceEndpointId::new(self.endpoint).map_err(|_| InvalidReplayOutput)?,
            SourceSignalId::new(self.signal).map_err(|_| InvalidReplayOutput)?,
            measurement,
            self.time.map(UnixTimeMillis::new),
        ))
    }
}
/// Closed, payload-free conversion failure.
#[derive(Clone, Copy, Debug)]
pub struct InvalidReplayOutput;

impl std::fmt::Display for InvalidReplayOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid bounded replay output")
    }
}
impl std::error::Error for InvalidReplayOutput {}
#[cfg(test)]
mod tests {
    use super::*;
    fn output(value: ReplayValue) -> ReplayOutput {
        ReplayOutput {
            endpoint: "main".into(),
            signal: "sample".into(),
            value,
            unit: None,
            time: Some(42),
            quality: "good".into(),
        }
    }
    #[test]
    fn exact_values_round_trip_but_unbounded_decimal_and_text_are_rejected() {
        for value in [
            ReplayValue::Integer(i64::MAX),
            ReplayValue::Decimal("0.125".into()),
            ReplayValue::Text("sample".into()),
            ReplayValue::Boolean(true),
        ] {
            let decoded = output(value).into_output().unwrap();
            assert_eq!(
                ReplayOutput::from_output(&decoded).into_output().unwrap(),
                decoded
            );
        }
        for value in [
            ReplayValue::Decimal("1e999999999".into()),
            ReplayValue::Decimal("0".repeat(257)),
            ReplayValue::Text("x".repeat(8193)),
        ] {
            assert!(output(value).into_output().is_err());
        }
    }
    #[test]
    fn execution_identity_is_nonzero_and_stable_in_one_binary() {
        assert_ne!(replay_fingerprint(), [0; 32]);
        assert_eq!(replay_fingerprint(), replay_fingerprint());
    }
}

//! Exact wire values: large integers, decimals, revisions and times are strings.
use super::cursor::{self, Interpretation};
use fleetiq_domain::PropertyValue;
use fleetiq_history::StoredPropertyChange;
use fleetiq_telemetry::ObservationQuality;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Parameters {
    pub(super) start_at_ms: String,
    pub(super) end_at_ms: String,
    pub(super) limit: Option<u16>,
    pub(super) after: Option<String>,
}
#[derive(Serialize)]
pub(super) struct HistoryPageBody {
    pub(super) changes: Vec<ChangeBody>,
    pub(super) next_after: Option<String>,
}
#[derive(Serialize)]
pub(crate) struct ChangeBody {
    interpretation: String,
    tenant_id: String,
    asset_id: String,
    property_id: String,
    property_version: u32,
    binding_id: String,
    binding_revision: String,
    event_time_basis: &'static str,
    event_at_ms: String,
    first_resolution_effective_at_ms: String,
    first_resolution_known_at_ms: String,
    value: ValueBody,
    source_unit: Option<String>,
    quality: &'static str,
    source: SourceBody,
}
#[derive(Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum ValueBody {
    Boolean(bool),
    Integer(String),
    Decimal(String),
    Text(String),
}
#[derive(Serialize)]
struct SourceBody {
    message_id: String,
    protocol_package_id: String,
    protocol_package_version: String,
    output_index: u32,
    device_asset_id: String,
    endpoint_id: String,
    signal_id: String,
    device_recorded_at_ms: Option<String>,
    received_at_ms: String,
}
impl ChangeBody {
    pub(crate) fn new(change: &StoredPropertyChange) -> Result<Self, serde_json::Error> {
        let source = change.observation();
        let id = change.id();
        Ok(Self {
            interpretation: cursor::encode(&Interpretation::from_id(id))?,
            tenant_id: change.tenant_id().as_str().to_owned(),
            asset_id: change.target().asset_id().as_str().to_owned(),
            property_id: change.target().property().id().as_str().to_owned(),
            property_version: change.target().property().version().get(),
            binding_id: id.binding_id().as_str().to_owned(),
            binding_revision: id.binding_revision().get().to_string(),
            event_time_basis: cursor::basis_name(id.event_time_basis()),
            event_at_ms: change.event_at().get().to_string(),
            first_resolution_effective_at_ms: change
                .first_resolution()
                .effective_at()
                .get()
                .to_string(),
            first_resolution_known_at_ms: change.first_resolution().known_at().get().to_string(),
            value: ValueBody::from(source.value()),
            source_unit: source.source_unit().map(|u| u.as_str().to_owned()),
            quality: match source.quality() {
                ObservationQuality::Good => "good",
                ObservationQuality::Uncertain => "uncertain",
                ObservationQuality::Invalid => "invalid",
            },
            source: SourceBody {
                message_id: source.id().message_id().as_str().to_owned(),
                protocol_package_id: source.decoded_by().id().as_str().to_owned(),
                protocol_package_version: source.decoded_by().version().as_str().to_owned(),
                output_index: source.id().output_index(),
                device_asset_id: source.source().device_asset_id().as_str().to_owned(),
                endpoint_id: source.source().endpoint_id().as_str().to_owned(),
                signal_id: source.source().signal_id().as_str().to_owned(),
                device_recorded_at_ms: source.device_recorded_at().map(|t| t.get().to_string()),
                received_at_ms: source.received_at().get().to_string(),
            },
        })
    }
}

impl From<&PropertyValue> for ValueBody {
    fn from(value: &PropertyValue) -> Self {
        match value {
            PropertyValue::Boolean(value) => Self::Boolean(*value),
            PropertyValue::Integer(value) => Self::Integer(value.to_string()),
            PropertyValue::Decimal(value) => Self::Decimal(value.to_string()),
            PropertyValue::Text(value) => Self::Text(value.clone()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn wire_values_preserve_kinds_and_precision() {
        let decimal = "12345678901234567890.1234567890123456789".parse().unwrap();
        for (value, expected) in [
            (
                PropertyValue::Integer(i64::MIN),
                json!({"kind":"integer","value":"-9223372036854775808"}),
            ),
            (
                PropertyValue::Decimal(decimal),
                json!({"kind":"decimal","value":"12345678901234567890.1234567890123456789"}),
            ),
            (
                PropertyValue::Boolean(false),
                json!({"kind":"boolean","value":false}),
            ),
            (
                PropertyValue::Text("<script>untrusted</script>".into()),
                json!({"kind":"text","value":"<script>untrusted</script>"}),
            ),
        ] {
            assert_eq!(
                serde_json::to_value(ValueBody::from(&value)).unwrap(),
                expected
            );
        }
    }
}

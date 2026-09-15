use bigdecimal::BigDecimal;
use fleetiq_application::DecodedObservationBatch;
use fleetiq_domain::{
    DeviceEndpointId, PropertyValue, SignalSource, SourceSignalId, UnitId, UnixTimeMillis,
};
use fleetiq_telemetry::{
    AcceptedMessage, DecodedMeasurement, DecodedObservation, ObservationQuality, ObservationTimes,
};

use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};

pub(super) struct DecodedResultRow {
    pub(super) observation_count: i64,
    pub(super) message_matches: bool,
}

impl DecodedResultRow {
    pub(super) async fn find(
        batch: &DecodedObservationBatch,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Option<Self>, PostgresRepositoryError> {
        let message = batch.message();
        let package = message.protocol_package();
        sqlx::query_as!(
            Self,
            r#"
            SELECT result.observation_count,
                   (message.device_asset_id = $5
                    AND message.received_at_ms = $6
                    AND message.payload = $7) AS "message_matches!"
            FROM ingestion.decoded_message_results AS result
            JOIN ingestion.accepted_messages AS message
              USING (tenant_id, message_id, protocol_package_id,
                     protocol_package_version)
            WHERE result.tenant_id = $1
              AND result.message_id = $2
              AND result.protocol_package_id = $3
              AND result.protocol_package_version = $4
            "#,
            message.tenant_id().as_str(),
            message.id().as_str(),
            package.id().as_str(),
            package.version().as_str(),
            message.device_asset_id().as_str(),
            message.received_at().get(),
            message.payload().as_bytes(),
        )
        .fetch_optional(&mut **transaction)
        .await
        .map_err(classify_database_error)
    }
}

pub(super) struct DecodedObservationRow {
    output_index: i64,
    endpoint_id: String,
    signal_id: String,
    device_recorded_at_ms: Option<i64>,
    value_kind: String,
    value_boolean: Option<bool>,
    value_integer: Option<i64>,
    value_decimal: Option<BigDecimal>,
    value_text: Option<String>,
    source_unit: Option<String>,
    quality: String,
}

impl DecodedObservationRow {
    pub(super) async fn find_all(
        batch: &DecodedObservationBatch,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Vec<Self>, PostgresRepositoryError> {
        let message = batch.message();
        let package = message.protocol_package();
        sqlx::query_as!(
            Self,
            r"
            SELECT output_index, endpoint_id, signal_id, device_recorded_at_ms,
                   value_kind, value_boolean, value_integer, value_decimal,
                   value_text, source_unit, quality
            FROM ingestion.decoded_observations
            WHERE tenant_id = $1
              AND message_id = $2
              AND protocol_package_id = $3
              AND protocol_package_version = $4
            ORDER BY output_index
            ",
            message.tenant_id().as_str(),
            message.id().as_str(),
            package.id().as_str(),
            package.version().as_str(),
        )
        .fetch_all(&mut **transaction)
        .await
        .map_err(classify_database_error)
    }

    pub(super) fn into_domain(
        self,
        message: &AcceptedMessage,
    ) -> Result<DecodedObservation, PostgresRepositoryError> {
        let output_index = u32::try_from(self.output_index)
            .map_err(|error| corrupt("decoded_observations.output_index", error))?;
        let value = self.reconstruct_value()?;
        let measurement = DecodedMeasurement::new(
            value,
            self.source_unit
                .map(UnitId::new)
                .transpose()
                .map_err(|error| corrupt("decoded_observations.source_unit", error))?,
            parse_quality(&self.quality)?,
        )
        .map_err(|error| corrupt("decoded_observations", error))?;

        Ok(DecodedObservation::new(
            message.decoded_observation_id(output_index),
            message.tenant_id().clone(),
            SignalSource::new(
                message.device_asset_id().clone(),
                DeviceEndpointId::new(self.endpoint_id)
                    .map_err(|error| corrupt("decoded_observations.endpoint_id", error))?,
                SourceSignalId::new(self.signal_id)
                    .map_err(|error| corrupt("decoded_observations.signal_id", error))?,
            ),
            measurement,
            ObservationTimes::new(
                self.device_recorded_at_ms.map(UnixTimeMillis::new),
                message.received_at(),
            ),
        ))
    }

    fn reconstruct_value(&self) -> Result<PropertyValue, PostgresRepositoryError> {
        match (
            self.value_kind.as_str(),
            self.value_boolean,
            self.value_integer,
            self.value_decimal.as_ref(),
            self.value_text.as_ref(),
        ) {
            ("boolean", Some(value), None, None, None) => Ok(PropertyValue::Boolean(value)),
            ("integer", None, Some(value), None, None) => Ok(PropertyValue::Integer(value)),
            ("decimal", None, None, Some(value), None) => Ok(PropertyValue::Decimal(value.clone())),
            ("text", None, None, None, Some(value)) => Ok(PropertyValue::Text(value.clone())),
            _ => Err(corrupt(
                "decoded_observations.value_kind",
                "typed value columns do not match the discriminator",
            )),
        }
    }
}

fn parse_quality(value: &str) -> Result<ObservationQuality, PostgresRepositoryError> {
    match value {
        "good" => Ok(ObservationQuality::Good),
        "uncertain" => Ok(ObservationQuality::Uncertain),
        "invalid" => Ok(ObservationQuality::Invalid),
        _ => Err(corrupt(
            "decoded_observations.quality",
            "unsupported observation quality",
        )),
    }
}

use bigdecimal::BigDecimal;
use fleetiq_domain::PropertyValue;
use fleetiq_ingestion::{DecodedObservationAppendOutcome, DecodedObservationBatch};

use super::{
    PostgresDecodedObservationLog, PostgresDecodedObservationLogError,
    row::{DecodedObservationRow, DecodedResultRow},
};
use crate::error::{classify_database_error, corrupt};

impl PostgresDecodedObservationLog {
    /// Appends one complete result or resolves an exact idempotent retry.
    ///
    /// The marker and its observations are committed in one transaction. Child
    /// rows use one set-based `UNNEST` insert, keeping a batch at two database
    /// round trips regardless of observation count. On marker conflict, later
    /// statements in the same `READ COMMITTED` transaction receive fresh
    /// snapshots and can see a concurrently committed immutable winner.
    pub(super) async fn append_batch(
        &self,
        batch: &DecodedObservationBatch,
    ) -> Result<DecodedObservationAppendOutcome, PostgresDecodedObservationLogError> {
        let mut transaction = self.pool.begin().await.map_err(classify_database_error)?;
        let outcome = Self::append_in_transaction(batch, &mut transaction).await?;
        transaction
            .commit()
            .await
            .map_err(classify_database_error)?;
        Ok(outcome)
    }
    /// Shares the complete result append with replay's checkpoint transaction.
    /// The caller owns commit/rollback; no partial marker may escape on failure.
    pub(in crate::ingestion) async fn append_in_transaction(
        batch: &DecodedObservationBatch,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<DecodedObservationAppendOutcome, PostgresDecodedObservationLogError> {
        let message = batch.message();
        let package = message.protocol_package();
        let observation_count = i64::try_from(batch.len())
            .map_err(|error| corrupt("decoded_message_results.observation_count", error))?;

        let inserted = sqlx::query_scalar!(
            r#"
            INSERT INTO ingestion.decoded_message_results
                (tenant_id, message_id, protocol_package_id,
                 protocol_package_version, observation_count)
            SELECT $1, $2, $3, $4, $5
            FROM ingestion.accepted_messages
            WHERE tenant_id = $1
              AND message_id = $2
              AND protocol_package_id = $3
              AND protocol_package_version = $4
              AND device_asset_id = $6
              AND received_at_ms = $7
              AND payload = $8
            ON CONFLICT
                (tenant_id, message_id, protocol_package_id,
                 protocol_package_version)
            DO NOTHING
            RETURNING TRUE AS "inserted!"
            "#,
            message.tenant_id().as_str(),
            message.id().as_str(),
            package.id().as_str(),
            package.version().as_str(),
            observation_count,
            message.device_asset_id().as_str(),
            message.received_at().get(),
            message.payload().as_bytes(),
        )
        .fetch_optional(&mut **transaction)
        .await
        .map_err(classify_database_error)?;

        if inserted.is_some() {
            if !batch.is_empty() {
                EncodedObservationColumns::from_batch(batch)
                    .insert(batch, transaction)
                    .await?;
            }
            return Ok(DecodedObservationAppendOutcome::Appended);
        }

        Self::resolve_existing(batch, transaction).await
    }

    async fn resolve_existing(
        batch: &DecodedObservationBatch,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<DecodedObservationAppendOutcome, PostgresDecodedObservationLogError> {
        let message = batch.message();
        let package = message.protocol_package();
        let Some(existing_result) = DecodedResultRow::find(batch, transaction).await? else {
            return Err(accepted_message_mismatch(batch));
        };
        if !existing_result.message_matches {
            return Err(accepted_message_mismatch(batch));
        }
        let existing_observations = DecodedObservationRow::find_all(batch, transaction).await?;
        let stored_count = usize::try_from(existing_result.observation_count)
            .map_err(|error| corrupt("decoded_message_results.observation_count", error))?;

        if existing_observations.len() != stored_count {
            return Err(corrupt(
                "decoded_message_results.observation_count",
                format_args!(
                    "marker declares {stored_count} observations but {} rows exist",
                    existing_observations.len()
                ),
            )
            .into());
        }

        let existing = DecodedObservationBatch::new(
            message.clone(),
            existing_observations
                .into_iter()
                .map(|row| row.into_domain(message))
                .collect::<Result<Vec<_>, _>>()?,
        )
        .map_err(|error| corrupt("decoded_observations", error))?;

        if &existing != batch {
            return Err(PostgresDecodedObservationLogError::IdentityCollision {
                tenant_id: message.tenant_id().clone(),
                message_id: message.id().clone(),
                protocol_package: package.clone(),
            });
        }

        Ok(DecodedObservationAppendOutcome::AlreadyPresent)
    }
}

fn accepted_message_mismatch(
    batch: &DecodedObservationBatch,
) -> PostgresDecodedObservationLogError {
    PostgresDecodedObservationLogError::AcceptedMessageMismatch {
        tenant_id: batch.message().tenant_id().clone(),
        message_id: batch.message().id().clone(),
    }
}

struct EncodedObservationColumns<'a> {
    output_indices: Vec<i64>,
    endpoint_ids: Vec<&'a str>,
    signal_ids: Vec<&'a str>,
    device_recorded_at_ms: Vec<Option<i64>>,
    value_kinds: Vec<&'static str>,
    boolean_values: Vec<Option<bool>>,
    integer_values: Vec<Option<i64>>,
    decimal_values: Vec<Option<BigDecimal>>,
    text_values: Vec<Option<&'a str>>,
    source_units: Vec<Option<&'a str>>,
    qualities: Vec<&'static str>,
}

impl<'a> EncodedObservationColumns<'a> {
    fn from_batch(batch: &'a DecodedObservationBatch) -> Self {
        let mut columns = Self {
            output_indices: Vec::with_capacity(batch.len()),
            endpoint_ids: Vec::with_capacity(batch.len()),
            signal_ids: Vec::with_capacity(batch.len()),
            device_recorded_at_ms: Vec::with_capacity(batch.len()),
            value_kinds: Vec::with_capacity(batch.len()),
            boolean_values: Vec::with_capacity(batch.len()),
            integer_values: Vec::with_capacity(batch.len()),
            decimal_values: Vec::with_capacity(batch.len()),
            text_values: Vec::with_capacity(batch.len()),
            source_units: Vec::with_capacity(batch.len()),
            qualities: Vec::with_capacity(batch.len()),
        };

        for observation in batch.observations() {
            columns
                .output_indices
                .push(i64::from(observation.id().output_index()));
            columns
                .endpoint_ids
                .push(observation.source().endpoint_id().as_str());
            columns
                .signal_ids
                .push(observation.source().signal_id().as_str());
            columns.device_recorded_at_ms.push(
                observation
                    .device_recorded_at()
                    .map(fleetiq_domain::UnixTimeMillis::get),
            );
            columns.push_value(observation.value());
            columns
                .source_units
                .push(observation.source_unit().map(AsRef::as_ref));
            columns.qualities.push(match observation.quality() {
                fleetiq_telemetry::ObservationQuality::Good => "good",
                fleetiq_telemetry::ObservationQuality::Uncertain => "uncertain",
                fleetiq_telemetry::ObservationQuality::Invalid => "invalid",
            });
        }

        columns
    }

    fn push_value(&mut self, value: &'a PropertyValue) {
        let (kind, boolean, integer, decimal, text) = match value {
            PropertyValue::Boolean(value) => ("boolean", Some(*value), None, None, None),
            PropertyValue::Integer(value) => ("integer", None, Some(*value), None, None),
            PropertyValue::Decimal(value) => ("decimal", None, None, Some(value.clone()), None),
            PropertyValue::Text(value) => ("text", None, None, None, Some(value.as_str())),
        };
        self.value_kinds.push(kind);
        self.boolean_values.push(boolean);
        self.integer_values.push(integer);
        self.decimal_values.push(decimal);
        self.text_values.push(text);
    }

    async fn insert(
        &self,
        batch: &DecodedObservationBatch,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), PostgresDecodedObservationLogError> {
        let message = batch.message();
        let package = message.protocol_package();

        sqlx::query!(
            r"
            INSERT INTO ingestion.decoded_observations
                (tenant_id, message_id, protocol_package_id,
                 protocol_package_version, output_index, endpoint_id, signal_id,
                 device_recorded_at_ms, value_kind, value_boolean, value_integer,
                 value_decimal, value_text, source_unit, quality)
            SELECT $1, $2, $3, $4, observation.*
            FROM UNNEST(
                $5::bigint[], $6::text[], $7::text[], $8::bigint[],
                $9::text[], $10::boolean[], $11::bigint[], $12::numeric[],
                $13::text[], $14::text[], $15::text[]
            ) AS observation(
                output_index, endpoint_id, signal_id, device_recorded_at_ms,
                value_kind, value_boolean, value_integer, value_decimal,
                value_text, source_unit, quality
            )
            ",
            message.tenant_id().as_str(),
            message.id().as_str(),
            package.id().as_str(),
            package.version().as_str(),
            &self.output_indices,
            &self.endpoint_ids as &[&str],
            &self.signal_ids as &[&str],
            &self.device_recorded_at_ms as &[Option<i64>],
            &self.value_kinds as &[&str],
            &self.boolean_values as &[Option<bool>],
            &self.integer_values as &[Option<i64>],
            &self.decimal_values as &[Option<BigDecimal>],
            &self.text_values as &[Option<&str>],
            &self.source_units as &[Option<&str>],
            &self.qualities as &[&str],
        )
        .execute(&mut **transaction)
        .await
        .map_err(classify_database_error)?;
        Ok(())
    }
}

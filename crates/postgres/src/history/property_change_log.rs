use std::{error::Error, fmt, future::Future};

use bigdecimal::BigDecimal;
use fleetiq_domain::PropertyValue;
use fleetiq_history::{AssetPropertyChangeLog, PropertyChangeAppendOutcome};
use fleetiq_semantics::{AssetPropertyChange, EventTimeBasis};
use fleetiq_telemetry::ObservationQuality;
use sqlx::PgPool;

use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};

/// PostgreSQL-backed, append-only semantic property-change history.
#[derive(Clone, Debug)]
pub struct PostgresAssetPropertyChangeLog {
    pool: PgPool,
}

impl PostgresAssetPropertyChangeLog {
    /// Uses an existing pool whose database has the complete `FleetIQ` migrations.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Checks the candidate against immutable source and semantic facts.
    ///
    /// Foreign keys and an insert trigger repeat essential lineage checks for
    /// direct SQL writers. This read additionally compares the caller's full
    /// decoded value, quality, timestamps, source, and target so a forged or
    /// stale in-memory candidate never receives an idempotent success result.
    pub(super) async fn matches_durable_facts(
        change: &AssetPropertyChange,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<bool, PostgresRepositoryError> {
        let observation = change.bound_observation().observation();
        let binding = change.bound_observation().binding();
        let context = change.bound_observation().context();
        let identity = observation.id();
        let package = identity.decoded_by();
        let target = change.target();
        let encoded = EncodedValue::from(change.value());
        let revision = BigDecimal::from(binding.revision().get());
        let matches = sqlx::query_scalar!(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM ingestion.decoded_observations AS observation
                JOIN ingestion.accepted_messages AS message
                  USING (tenant_id, message_id, protocol_package_id,
                         protocol_package_version)
                JOIN metadata.signal_binding_revisions AS binding
                  ON binding.tenant_id = observation.tenant_id
                 AND binding.binding_id = $18
                 AND binding.revision = $19
                JOIN metadata.property_definitions AS property
                  ON property.tenant_id = binding.tenant_id
                 AND property.property_id = binding.target_property_id
                 AND property.version = binding.target_property_version
                WHERE observation.tenant_id = $1
                  AND observation.message_id = $2
                  AND observation.protocol_package_id = $3
                  AND observation.protocol_package_version = $4
                  AND observation.output_index = $5
                  AND message.device_asset_id = $6
                  AND message.received_at_ms = $7
                  AND observation.endpoint_id = $8
                  AND observation.signal_id = $9
                  AND observation.device_recorded_at_ms IS NOT DISTINCT FROM $10::bigint
                  AND observation.value_kind = $11
                  AND observation.value_boolean IS NOT DISTINCT FROM $12::boolean
                  AND observation.value_integer IS NOT DISTINCT FROM $13::bigint
                  AND observation.value_decimal IS NOT DISTINCT FROM $14::numeric
                  AND observation.value_text IS NOT DISTINCT FROM $15::text
                  AND observation.source_unit IS NOT DISTINCT FROM $16::text
                  AND observation.quality = $17
                  AND binding.source_device_asset_id = message.device_asset_id
                  AND binding.source_endpoint_id = observation.endpoint_id
                  AND binding.source_signal_id = observation.signal_id
                  AND binding.target_asset_id = $20
                  AND binding.target_property_id = $21
                  AND binding.target_property_version = $22
                  AND property.value_kind = observation.value_kind
                  AND property.canonical_unit IS NOT DISTINCT FROM observation.source_unit
                  AND binding.recorded_at_ms <= $23
                  AND binding.effective_start_ms <= $24
                  AND (binding.effective_end_ms IS NULL
                       OR $24 < binding.effective_end_ms)
                  AND NOT EXISTS (
                      SELECT 1
                      FROM metadata.signal_binding_revisions AS later
                      WHERE later.tenant_id = binding.tenant_id
                        AND later.binding_id = binding.binding_id
                        AND later.revision > binding.revision
                        AND later.recorded_at_ms <= $23
                  )
            ) AS "matches!"
            "#,
            change.tenant_id().as_str(),
            identity.message_id().as_str(),
            package.id().as_str(),
            package.version().as_str(),
            i64::from(identity.output_index()),
            observation.source().device_asset_id().as_str(),
            observation.received_at().get(),
            observation.source().endpoint_id().as_str(),
            observation.source().signal_id().as_str(),
            observation
                .device_recorded_at()
                .map(fleetiq_domain::UnixTimeMillis::get),
            encoded.kind,
            encoded.boolean,
            encoded.integer,
            encoded.decimal,
            encoded.text,
            observation.source_unit().map(AsRef::as_ref),
            quality_name(observation.quality()),
            binding.binding_id().as_str(),
            revision,
            target.asset_id().as_str(),
            target.property().id().as_str(),
            i64::from(target.property().version().get()),
            context.known_at().get(),
            context.effective_at().get(),
        )
        .fetch_one(&mut **transaction)
        .await
        .map_err(classify_database_error)?;
        Ok(matches)
    }

    async fn append_change(
        &self,
        change: &AssetPropertyChange,
    ) -> Result<PropertyChangeAppendOutcome, PostgresAssetPropertyChangeLogError> {
        let mut transaction = self.pool.begin().await.map_err(classify_database_error)?;
        let outcome = Self::append_in_transaction(change, &mut transaction).await?;
        transaction
            .commit()
            .await
            .map_err(classify_database_error)?;
        Ok(outcome)
    }

    /// Shares lineage validation and idempotent append with fenced work completion.
    /// The caller owns commit; errors roll back both history and progress.
    pub(super) async fn append_in_transaction(
        change: &AssetPropertyChange,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<PropertyChangeAppendOutcome, PostgresAssetPropertyChangeLogError> {
        if !Self::matches_durable_facts(change, transaction).await? {
            return Err(PostgresAssetPropertyChangeLogError::SourceMismatch);
        }
        let id = change.id();
        let observation = id.observation_id();
        let package = observation.decoded_by();
        let context = change.bound_observation().context();
        let binding_revision = BigDecimal::from(id.binding_revision().get());
        let basis = basis_name(id.event_time_basis());
        let inserted = sqlx::query_scalar!(
            r#"
            INSERT INTO history.asset_property_changes
                (tenant_id, message_id, protocol_package_id,
                 protocol_package_version, output_index, binding_id,
                 binding_revision, event_time_basis, event_at_ms,
                 resolution_effective_at_ms, resolution_known_at_ms)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT ON CONSTRAINT pk_asset_property_changes DO NOTHING
            RETURNING TRUE AS "inserted!"
            "#,
            change.tenant_id().as_str(),
            observation.message_id().as_str(),
            package.id().as_str(),
            package.version().as_str(),
            i64::from(observation.output_index()),
            id.binding_id().as_str(),
            binding_revision,
            basis,
            change.event_at().get(),
            context.effective_at().get(),
            context.known_at().get(),
        )
        .fetch_optional(&mut **transaction)
        .await
        .map_err(classify_database_error)?;
        if inserted.is_some() {
            return Ok(PropertyChangeAppendOutcome::Appended);
        }

        // A new READ COMMITTED statement sees a concurrent winner after the
        // uniqueness check waits. Context cutoffs may differ on exact semantic
        // replay; immutable parents and event time define equal content.
        let stored_event_at = sqlx::query_scalar!(
            r"
            SELECT event_at_ms
            FROM history.asset_property_changes
            WHERE tenant_id = $1 AND message_id = $2
              AND protocol_package_id = $3 AND protocol_package_version = $4
              AND output_index = $5 AND binding_id = $6
              AND binding_revision = $7 AND event_time_basis = $8
            ",
            change.tenant_id().as_str(),
            observation.message_id().as_str(),
            package.id().as_str(),
            package.version().as_str(),
            i64::from(observation.output_index()),
            id.binding_id().as_str(),
            BigDecimal::from(id.binding_revision().get()),
            basis,
        )
        .fetch_optional(&mut **transaction)
        .await
        .map_err(classify_database_error)?
        .ok_or_else(|| corrupt("history.asset_property_changes", "missing conflict winner"))?;
        if stored_event_at != change.event_at().get() {
            return Err(PostgresAssetPropertyChangeLogError::IdentityCollision);
        }
        Ok(PropertyChangeAppendOutcome::AlreadyPresent)
    }
}

impl AssetPropertyChangeLog for PostgresAssetPropertyChangeLog {
    type Error = PostgresAssetPropertyChangeLogError;

    fn append<'a>(
        &'a self,
        change: &'a AssetPropertyChange,
    ) -> impl Future<Output = Result<PropertyChangeAppendOutcome, Self::Error>> + Send + 'a {
        self.append_change(change)
    }
}

/// Failure while verifying or appending a semantic property change.
#[derive(Debug)]
pub enum PostgresAssetPropertyChangeLogError {
    /// Supplied source or binding content disagrees with durable facts.
    SourceMismatch,
    /// An existing interpretation identity has a different event time.
    IdentityCollision,
    /// PostgreSQL failed or persisted data was inconsistent.
    Repository(PostgresRepositoryError),
}

impl fmt::Display for PostgresAssetPropertyChangeLogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceMismatch => {
                formatter.write_str("semantic source provenance does not match durable facts")
            }
            Self::IdentityCollision => {
                formatter.write_str("semantic change identity has different immutable content")
            }
            Self::Repository(error) => error.fmt(formatter),
        }
    }
}

impl Error for PostgresAssetPropertyChangeLogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(error) => Some(error),
            Self::SourceMismatch | Self::IdentityCollision => None,
        }
    }
}

impl From<PostgresRepositoryError> for PostgresAssetPropertyChangeLogError {
    fn from(error: PostgresRepositoryError) -> Self {
        Self::Repository(error)
    }
}

pub(super) fn basis_name(basis: EventTimeBasis) -> &'static str {
    match basis {
        EventTimeBasis::DeviceRecorded => "device_recorded",
        EventTimeBasis::ReceivedAt => "received_at",
    }
}

fn quality_name(quality: ObservationQuality) -> &'static str {
    match quality {
        ObservationQuality::Good => "good",
        ObservationQuality::Uncertain => "uncertain",
        ObservationQuality::Invalid => "invalid",
    }
}

struct EncodedValue<'a> {
    kind: &'static str,
    boolean: Option<bool>,
    integer: Option<i64>,
    decimal: Option<BigDecimal>,
    text: Option<&'a str>,
}

impl<'a> From<&'a PropertyValue> for EncodedValue<'a> {
    fn from(value: &'a PropertyValue) -> Self {
        match value {
            PropertyValue::Boolean(value) => Self {
                kind: "boolean",
                boolean: Some(*value),
                integer: None,
                decimal: None,
                text: None,
            },
            PropertyValue::Integer(value) => Self {
                kind: "integer",
                boolean: None,
                integer: Some(*value),
                decimal: None,
                text: None,
            },
            PropertyValue::Decimal(value) => Self {
                kind: "decimal",
                boolean: None,
                integer: None,
                decimal: Some(value.clone()),
                text: None,
            },
            PropertyValue::Text(value) => Self {
                kind: "text",
                boolean: None,
                integer: None,
                decimal: None,
                text: Some(value.as_str()),
            },
        }
    }
}

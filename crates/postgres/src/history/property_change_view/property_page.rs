//! Set-based target history, with shared source and interpretation reconstruction.
use super::{PostgresAssetPropertyChangeView, StoredChangeRow};
use crate::{
    error::{PostgresRepositoryError, classify_database_error, corrupt},
    history::decoded_observation_view::StoredObservationRow,
};
use bigdecimal::BigDecimal;
use fleetiq_domain::{RecordRevision, SignalBindingId, TenantId};
use fleetiq_history::{
    PropertyHistoryCursor, PropertyHistoryPage, PropertyHistoryQuery, PropertyHistoryView,
};
use fleetiq_semantics::{AssetPropertyChangeId, EventTimeBasis};
use fleetiq_telemetry::{
    AcceptedMessageId, DecodedObservationId, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};

impl PropertyHistoryView for PostgresAssetPropertyChangeView {
    async fn list_property(
        &self,
        tenant: &TenantId,
        query: &PropertyHistoryQuery,
    ) -> Result<PropertyHistoryPage, PostgresRepositoryError> {
        let cursor = query.after();
        let id = cursor.map(PropertyHistoryCursor::id);
        let source = id.map(AssetPropertyChangeId::observation_id);
        // Empty identifiers, output -1 and revision 0 form a private lower key
        // before every valid interpretation at start. This keeps the range
        // predicate indexable without an optional-cursor OR or OFFSET scan.
        let mut tx = self.pool.begin().await.map_err(classify_database_error)?;
        // One expensive fan-out must not monopolize a pooled database session.
        // SET LOCAL is restored on commit, error, timeout and cancellation.
        sqlx::query("SET LOCAL statement_timeout = '5s'")
            .execute(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        let mut rows = sqlx::query_file_as!(
            HistoryRow,
            "src/history/property_change_view/property_page.sql",
            tenant.as_str(),
            query.target().asset_id().as_str(),
            query.target().property().id().as_str(),
            i64::from(query.target().property().version().get()),
            query.start().get(),
            query.end().get(),
            cursor.map_or(query.start().get(), |c| c.event_at().get()),
            source.map_or("", |s| s.message_id().as_str()),
            source.map_or("", |s| s.decoded_by().id().as_str()),
            source.map_or("", |s| s.decoded_by().version().as_str()),
            source.map_or(-1, |s| i64::from(s.output_index())),
            id.map_or("", |i| i.binding_id().as_str()),
            id.map_or_else(
                || BigDecimal::from(0),
                |i| BigDecimal::from(i.binding_revision().get())
            ),
            id.map_or("", |i| super::basis_name(i.event_time_basis())),
            i64::from(query.limit()) + 1,
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        tx.commit().await.map_err(classify_database_error)?;
        let more = rows.len() > usize::from(query.limit());
        rows.truncate(usize::from(query.limit()));
        let changes = rows
            .into_iter()
            .map(|row| row.into_domain(tenant))
            .collect::<Result<Vec<_>, _>>()?;
        let next = if more {
            changes
                .last()
                .map(|c| PropertyHistoryCursor::new(c.event_at(), c.id().clone()))
        } else {
            None
        };
        Ok(PropertyHistoryPage::new(changes, next))
    }
}

/// Flat checked SQL row: no JSON aggregation or per-result source lookup.
struct HistoryRow {
    message_id: String,
    protocol_package_id: String,
    protocol_package_version: String,
    output_index: i64,
    binding_id: String,
    binding_revision: BigDecimal,
    event_time_basis: String,
    event_at_ms: i64,
    resolution_effective_at_ms: i64,
    resolution_known_at_ms: i64,
    target_asset_id: String,
    target_property_id: String,
    target_property_version: i64,
    device_asset_id: String,
    received_at_ms: i64,
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
impl HistoryRow {
    fn into_domain(
        self,
        tenant: &TenantId,
    ) -> Result<fleetiq_history::StoredPropertyChange, PostgresRepositoryError> {
        let source_id = DecodedObservationId::new(
            AcceptedMessageId::new(self.message_id)
                .map_err(|e| corrupt("history.property_page.identity", e))?,
            ProtocolPackageRef::new(
                ProtocolPackageId::new(self.protocol_package_id)
                    .map_err(|e| corrupt("history.property_page.identity", e))?,
                ProtocolPackageVersion::new(self.protocol_package_version)
                    .map_err(|e| corrupt("history.property_page.identity", e))?,
            ),
            u32::try_from(self.output_index)
                .map_err(|e| corrupt("history.property_page.output_index", e))?,
        );
        let basis = match self.event_time_basis.as_str() {
            "device_recorded" => EventTimeBasis::DeviceRecorded,
            "received_at" => EventTimeBasis::ReceivedAt,
            _ => {
                return Err(corrupt(
                    "history.property_page.event_time_basis",
                    "unknown time basis",
                ));
            }
        };
        let revision = self
            .binding_revision
            .to_string()
            .parse::<u64>()
            .map_err(|e| corrupt("history.property_page.binding_revision", e))?;
        let id = AssetPropertyChangeId::from_parts(
            source_id.clone(),
            SignalBindingId::new(self.binding_id)
                .map_err(|e| corrupt("history.property_page.identity", e))?,
            RecordRevision::new(revision)
                .map_err(|e| corrupt("history.property_page.binding_revision", e))?,
            basis,
        );
        let observation = StoredObservationRow {
            device_asset_id: self.device_asset_id,
            received_at_ms: self.received_at_ms,
            endpoint_id: self.endpoint_id,
            signal_id: self.signal_id,
            device_recorded_at_ms: self.device_recorded_at_ms,
            value_kind: self.value_kind,
            value_boolean: self.value_boolean,
            value_integer: self.value_integer,
            value_decimal: self.value_decimal,
            value_text: self.value_text,
            source_unit: self.source_unit,
            quality: self.quality,
        }
        .into_domain(tenant, &source_id)?;
        StoredChangeRow {
            event_at_ms: self.event_at_ms,
            resolution_effective_at_ms: self.resolution_effective_at_ms,
            resolution_known_at_ms: self.resolution_known_at_ms,
            target_asset_id: self.target_asset_id,
            target_property_id: self.target_property_id,
            target_property_version: self.target_property_version,
        }
        .into_domain(&id, observation)
    }
}

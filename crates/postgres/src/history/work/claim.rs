//! Claims use the database clock and monotonically increasing attempt fences.
use super::{PostgresSemanticWorkError, PostgresSemanticWorkRepository};
use crate::{PostgresSignalBindingRepository, error::classify_database_error};
use bigdecimal::BigDecimal;
use fleetiq_domain::{RecordRevision, SignalBindingId, TenantId, UnixTimeMillis};
use fleetiq_history::{SemanticLease, StoredObservationRequest};
use fleetiq_metadata::SignalBindingRepository;
use fleetiq_semantics::EventTimeBasis;
use fleetiq_telemetry::{
    AcceptedMessageId, DecodedObservationId, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};

impl PostgresSemanticWorkRepository {
    pub(super) async fn claim_one(
        &self,
    ) -> Result<Option<SemanticLease>, PostgresSemanticWorkError> {
        let mut tx = self.pool.begin().await.map_err(classify_database_error)?;
        // Exhaustion is bounded too; retain evidence instead of spinning forever.
        sqlx::query!(r#"
            UPDATE history.semantic_work w SET status='exhausted'
            FROM (SELECT tenant_id,message_id,protocol_package_id,protocol_package_version,output_index
                FROM history.semantic_work WHERE status='pending' AND generation=3
                AND lease_until_ms<=floor(extract(epoch FROM clock_timestamp())*1000)::bigint
                ORDER BY tenant_id,message_id,protocol_package_id,protocol_package_version,output_index
                FOR UPDATE SKIP LOCKED LIMIT 32) x
            WHERE (w.tenant_id,w.message_id,w.protocol_package_id,w.protocol_package_version,w.output_index)
                = (x.tenant_id,x.message_id,x.protocol_package_id,x.protocol_package_version,x.output_index)
        "#).execute(&mut *tx).await.map_err(classify_database_error)?;
        let row = sqlx::query_as!(WorkRow, r#"
            WITH candidate AS (
                SELECT w.tenant_id,w.message_id,w.protocol_package_id,w.protocol_package_version,w.output_index
                FROM history.semantic_work w JOIN metadata.tenants t USING(tenant_id)
                WHERE w.status='pending' AND w.generation<3 AND t.is_active
                  AND (w.lease_until_ms IS NULL OR w.lease_until_ms<=floor(extract(epoch FROM clock_timestamp())*1000)::bigint)
                ORDER BY w.tenant_id,w.message_id,w.protocol_package_id,w.protocol_package_version,w.output_index
                FOR UPDATE OF w SKIP LOCKED LIMIT 1
            ) UPDATE history.semantic_work w SET generation=generation+1,
                lease_until_ms=floor(extract(epoch FROM clock_timestamp())*1000)::bigint+30000
            FROM candidate c WHERE
                (w.tenant_id,w.message_id,w.protocol_package_id,w.protocol_package_version,w.output_index)
                = (c.tenant_id,c.message_id,c.protocol_package_id,c.protocol_package_version,c.output_index)
            RETURNING w.tenant_id,w.message_id,w.protocol_package_id,w.protocol_package_version,
                w.output_index,w.event_time_basis,w.known_at_ms,w.generation,w.binding_id,w.binding_revision
        "#).fetch_optional(&mut *tx).await.map_err(classify_database_error)?;
        tx.commit().await.map_err(classify_database_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let request = row.request()?;
        let selected = if let (Some(id), Some(revision)) = (row.binding_id, row.binding_revision) {
            let id = SignalBindingId::new(id).map_err(|_| PostgresSemanticWorkError::Invalid)?;
            let number = revision
                .to_string()
                .parse()
                .map_err(|_| PostgresSemanticWorkError::Invalid)?;
            let revision =
                RecordRevision::new(number).map_err(|_| PostgresSemanticWorkError::Invalid)?;
            Some(
                PostgresSignalBindingRepository::new(self.pool.clone())
                    .find_revision(request.tenant_id(), &id, revision)
                    .await?
                    .ok_or(PostgresSemanticWorkError::Invalid)?,
            )
        } else {
            None
        };
        Ok(Some(SemanticLease {
            request,
            generation: row.generation,
            selected,
        }))
    }
}
struct WorkRow {
    tenant_id: String,
    message_id: String,
    protocol_package_id: String,
    protocol_package_version: String,
    output_index: i64,
    event_time_basis: String,
    known_at_ms: i64,
    generation: i32,
    binding_id: Option<String>,
    binding_revision: Option<BigDecimal>,
}
impl WorkRow {
    fn request(&self) -> Result<StoredObservationRequest, PostgresSemanticWorkError> {
        let tenant = TenantId::new(self.tenant_id.clone())
            .map_err(|_| PostgresSemanticWorkError::Invalid)?;
        let message = AcceptedMessageId::new(self.message_id.clone())
            .map_err(|_| PostgresSemanticWorkError::Invalid)?;
        let package = ProtocolPackageRef::new(
            ProtocolPackageId::new(self.protocol_package_id.clone())
                .map_err(|_| PostgresSemanticWorkError::Invalid)?,
            ProtocolPackageVersion::new(self.protocol_package_version.clone())
                .map_err(|_| PostgresSemanticWorkError::Invalid)?,
        );
        let output =
            u32::try_from(self.output_index).map_err(|_| PostgresSemanticWorkError::Invalid)?;
        let basis = match self.event_time_basis.as_str() {
            "device_recorded" => EventTimeBasis::DeviceRecorded,
            "received_at" => EventTimeBasis::ReceivedAt,
            _ => return Err(PostgresSemanticWorkError::Invalid),
        };
        Ok(StoredObservationRequest::new(
            tenant,
            DecodedObservationId::new(message, package, output),
            UnixTimeMillis::new(self.known_at_ms),
            basis,
        ))
    }
}

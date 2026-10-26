//! Reconstruct immutable context through validating domain constructors.
use super::PostgresReplayError;
use crate::error::{classify_database_error, corrupt};
use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use fleetiq_ingestion::{ProtocolAdmissionWorld, ReplayWork};
use fleetiq_telemetry::{
    AcceptedMessage, AcceptedMessageId, AcceptedPayload, ProtocolArtifactDigest, ProtocolPackageId,
    ProtocolPackageRef, ProtocolPackageVersion,
};
pub(super) struct WorkRow {
    tenant_id: String,
    message_id: String,
    device_asset_id: String,
    protocol_package_id: String,
    protocol_package_version: String,
    received_at_ms: i64,
    payload: Vec<u8>,
    session_id: String,
    sequence: i64,
    artifact_sha256: Vec<u8>,
    wit_world: String,
    execution_sha256: Vec<u8>,
    pre_state: Vec<u8>,
    pub(super) generation: i64,
}
impl WorkRow {
    pub(super) async fn find(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        tenant: &str,
        message: &str,
    ) -> Result<Option<Self>, PostgresReplayError> {
        Ok(sqlx::query_as!(Self,r#"
            SELECT m.tenant_id,m.message_id,m.device_asset_id,m.protocol_package_id,m.protocol_package_version,m.received_at_ms,m.payload,w.session_id,w.sequence,s.artifact_sha256,s.wit_world,s.execution_sha256,w.pre_state,w.generation
            FROM ingestion.replay_work w
            JOIN ingestion.replay_sessions s USING(tenant_id,session_id)
            JOIN ingestion.accepted_messages m ON m.tenant_id=w.tenant_id
            AND m.message_id=w.message_id
            WHERE w.tenant_id=$1
            AND w.message_id=$2
            AND s.execution_policy=1
            AND s.admission_policy=1
            "#,tenant,message).fetch_optional(&mut **tx).await.map_err(classify_database_error)?)
    }
    pub(super) fn into_work(self) -> Result<ReplayWork, PostgresReplayError> {
        let message = AcceptedMessage::new(
            AcceptedMessageId::new(self.message_id).map_err(|e| corrupt("replay_work", e))?,
            TenantId::new(self.tenant_id).map_err(|e| corrupt("replay_work", e))?,
            AssetId::new(self.device_asset_id).map_err(|e| corrupt("replay_work", e))?,
            ProtocolPackageRef::new(
                ProtocolPackageId::new(self.protocol_package_id)
                    .map_err(|e| corrupt("replay_work", e))?,
                ProtocolPackageVersion::new(self.protocol_package_version)
                    .map_err(|e| corrupt("replay_work", e))?,
            ),
            UnixTimeMillis::new(self.received_at_ms),
            AcceptedPayload::new(self.payload),
        );
        let digest = ProtocolArtifactDigest::from_sha256(
            self.artifact_sha256
                .try_into()
                .map_err(|_| PostgresReplayError::Invalid)?,
        );
        let world = match self.wit_world.as_str() {
            "runtime-v1" => ProtocolAdmissionWorld::Runtime,
            "identity-v1" => ProtocolAdmissionWorld::Identity,
            _ => return Err(PostgresReplayError::Invalid),
        };
        let work = ReplayWork {
            message,
            session: self.session_id,
            sequence: u32::try_from(self.sequence).map_err(|_| PostgresReplayError::Invalid)?,
            digest,
            world,
            execution: self
                .execution_sha256
                .try_into()
                .map_err(|_| PostgresReplayError::Invalid)?,
            pre_state: self.pre_state,
        };
        work.validate().map_err(|_| PostgresReplayError::Invalid)?;
        Ok(work)
    }
}

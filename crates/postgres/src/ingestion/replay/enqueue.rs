//! Raw acceptance and replay input share a commit; no ACK escapes this adapter.
use super::{PostgresReplayError, PostgresReplayRepository, row::WorkRow};
use crate::error::classify_database_error;
use fleetiq_ingestion::ReplayWork;
impl PostgresReplayRepository {
    pub(super) async fn enqueue_work(
        &self,
        work: &ReplayWork,
    ) -> Result<bool, PostgresReplayError> {
        work.validate().map_err(|_| PostgresReplayError::Invalid)?;
        let m = &work.message;
        let t = m.tenant_id().as_str();
        let id = m.id().as_str();
        let p = m.protocol_package();
        let seq = i64::from(work.sequence);
        let mut tx = self.pool.begin().await.map_err(classify_database_error)?;
        if sqlx::query_scalar!(
            "SELECT is_active FROM metadata.tenants WHERE tenant_id=$1 FOR SHARE",
            t
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(classify_database_error)?
            != Some(true)
        {
            return Ok(false);
        }
        sqlx::query!(r#"
            INSERT INTO ingestion.replay_sessions(tenant_id,session_id,device_asset_id,protocol_package_id,protocol_package_version,artifact_sha256,wit_world,admission_policy,execution_policy,execution_sha256) VALUES($1,$2,$3,$4,$5,$6,$7,1,1,$8)
            ON CONFLICT DO NOTHING
            "#,t,&work.session,m.device_asset_id().as_str(),p.id().as_str(),p.version().as_str(),work.digest.as_bytes().as_slice(),work.world.as_str(),work.execution.as_slice()).execute(&mut *tx).await.map_err(classify_database_error)?;
        let s = sqlx::query!(
            r#"
            SELECT current_sequence,state,(device_asset_id=$3
            AND protocol_package_id=$4
            AND protocol_package_version=$5
            AND artifact_sha256=$6
            AND wit_world=$7
            AND admission_policy=1
            AND execution_policy=1
            AND execution_sha256=$8) AS "matches!"
            FROM ingestion.replay_sessions
            WHERE tenant_id=$1
            AND session_id=$2 FOR UPDATE
            "#,
            t,
            &work.session,
            m.device_asset_id().as_str(),
            p.id().as_str(),
            p.version().as_str(),
            work.digest.as_bytes().as_slice(),
            work.world.as_str(),
            work.execution.as_slice()
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        if !s.matches {
            return Ok(false);
        }
        if let Some(existing) = WorkRow::find(&mut tx, t, id).await? {
            return Ok(existing.into_work()? == *work);
        }
        if s.current_sequence + 1 != seq || s.state != work.pre_state {
            return Ok(false);
        }
        sqlx::query!(r#"
            INSERT INTO ingestion.accepted_messages(tenant_id,message_id,device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload) VALUES($1,$2,$3,$4,$5,$6,$7)
            ON CONFLICT DO NOTHING
            "#,t,id,m.device_asset_id().as_str(),p.id().as_str(),p.version().as_str(),m.received_at().get(),m.payload().as_bytes()).execute(&mut *tx).await.map_err(classify_database_error)?;
        let exact = sqlx::query_scalar!(
            r#"
            SELECT EXISTS(SELECT 1
            FROM ingestion.accepted_messages
            WHERE tenant_id=$1
            AND message_id=$2
            AND device_asset_id=$3
            AND protocol_package_id=$4
            AND protocol_package_version=$5
            AND received_at_ms=$6
            AND payload=$7) AS "exact!"
            "#,
            t,
            id,
            m.device_asset_id().as_str(),
            p.id().as_str(),
            p.version().as_str(),
            m.received_at().get(),
            m.payload().as_bytes()
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        if !exact {
            return Ok(false);
        }
        let inserted=sqlx::query!(r#"
            INSERT INTO ingestion.replay_work(tenant_id,message_id,session_id,sequence,pre_state) VALUES($1,$2,$3,$4,$5)
            ON CONFLICT DO NOTHING
            "#,t,id,&work.session,seq,&work.pre_state).execute(&mut *tx).await.map_err(classify_database_error)?.rows_affected()==1;
        if !inserted {
            return Ok(false);
        }
        tx.commit().await.map_err(classify_database_error)?;
        Ok(true)
    }
}

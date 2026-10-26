//! Result marker, all observations, work receipt and checkpoint commit together.
use super::{PostgresReplayError, PostgresReplayRepository, row::WorkRow};
use crate::{PostgresDecodedObservationLog, error::classify_database_error};
use fleetiq_ingestion::{
    DecodedObservationBatch, REPLAY_OUTPUT_LIMIT, REPLAY_STATE_LIMIT, ReplayLease,
};
impl PostgresReplayRepository {
    pub(super) async fn finish(
        &self,
        lease: &ReplayLease,
        batch: &DecodedObservationBatch,
        state: &[u8],
    ) -> Result<bool, PostgresReplayError> {
        if batch.message() != &lease.work.message
            || state.len() > REPLAY_STATE_LIMIT
            || batch.len() > REPLAY_OUTPUT_LIMIT
        {
            return Err(PostgresReplayError::Invalid);
        }
        let t = lease.work.message.tenant_id().as_str();
        let id = lease.work.message.id().as_str();
        let mut tx = self.pool.begin().await.map_err(classify_database_error)?;
        sqlx::query!("SELECT current_sequence FROM ingestion.replay_sessions WHERE tenant_id=$1 AND session_id=$2 FOR UPDATE",t,&lease.work.session).fetch_one(&mut *tx).await.map_err(classify_database_error)?;
        let Some(row) = sqlx::query!(
            r#"
            SELECT status,generation,next_state,(lease_until_ms>floor(extract(epoch FROM clock_timestamp())*1000)::bigint) AS "valid"
            FROM ingestion.replay_work
            WHERE tenant_id=$1
            AND message_id=$2 FOR UPDATE
            "#,
            t,
            id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(classify_database_error)?
        else {
            return Ok(false);
        };
        if row.generation != lease.generation || row.status == "poisoned" {
            return Ok(false);
        }
        let durable = WorkRow::find(&mut tx, t, id)
            .await?
            .ok_or(PostgresReplayError::Invalid)?
            .into_work()?;
        if durable != lease.work {
            return Ok(false);
        }
        let complete = row.status == "completed";
        if (complete && row.next_state.as_deref() != Some(state))
            || (!complete && row.valid != Some(true))
        {
            return Ok(false);
        }
        PostgresDecodedObservationLog::append_in_transaction(batch, &mut tx).await?;
        if !complete {
            sqlx::query!("UPDATE ingestion.replay_work SET status='completed',next_state=$3 WHERE tenant_id=$1 AND message_id=$2",t,id,state).execute(&mut *tx).await.map_err(classify_database_error)?;
            sqlx::query!("UPDATE ingestion.replay_sessions SET current_sequence=$3,state=$4 WHERE tenant_id=$1 AND session_id=$2",t,&lease.work.session,i64::from(lease.work.sequence),state).execute(&mut *tx).await.map_err(classify_database_error)?;
        }
        tx.commit().await.map_err(classify_database_error)?;
        Ok(true)
    }
}

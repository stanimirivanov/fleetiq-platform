//! Database-clock claims avoid worker clock skew; SKIP LOCKED permits contention.
use super::{PostgresReplayError, PostgresReplayRepository, row::WorkRow};
use crate::error::classify_database_error;
use fleetiq_ingestion::ReplayLease;
impl PostgresReplayRepository {
    pub(super) async fn claim_work(
        &self,
        exact: Option<(&str, &str)>,
    ) -> Result<Option<ReplayLease>, PostgresReplayError> {
        let (tenant, message) = exact.map_or((None, None), |(t, m)| (Some(t), Some(m)));
        let mut tx = self.pool.begin().await.map_err(classify_database_error)?;
        let execution = fleetiq_protocol_sdk::replay_wire::replay_fingerprint();
        // An abandoned final attempt becomes terminal; it must not disappear
        // silently from diagnostics or permit a later session turn to skip it.
        sqlx::query!(
            r#"
            UPDATE ingestion.replay_work w
            SET status='poisoned'
            FROM (SELECT w.tenant_id,w.message_id
            FROM ingestion.replay_work w
            JOIN ingestion.replay_sessions s USING(tenant_id,session_id)
            WHERE s.execution_sha256=$1
            AND w.status='pending'
            AND ($2::text IS NULL OR w.tenant_id=$2)
            AND ($3::text IS NULL OR w.message_id=$3)
            AND w.generation=3
            AND lease_until_ms<=floor(extract(epoch FROM clock_timestamp())*1000)::bigint
            ORDER BY lease_until_ms
            FOR UPDATE OF w SKIP LOCKED
            LIMIT 32) expired
            WHERE w.tenant_id=expired.tenant_id
            AND w.message_id=expired.message_id
            "#,
            execution.as_slice(),
            tenant,
            message
        )
        .execute(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        let candidate = sqlx::query!(
            r#"
            SELECT w.tenant_id,w.message_id
            FROM ingestion.replay_work w
            JOIN ingestion.replay_sessions s USING(tenant_id,session_id)
            WHERE s.execution_sha256=$1
            AND s.execution_policy=1
            AND s.admission_policy=1
            AND w.status='pending'
            AND w.generation<3
            AND (w.lease_until_ms IS NULL OR w.lease_until_ms<=floor(extract(epoch FROM clock_timestamp())*1000)::bigint)
            AND w.sequence=s.current_sequence+1
            AND ($2::text IS NULL OR w.tenant_id=$2)
            AND ($3::text IS NULL OR w.message_id=$3)
            ORDER BY w.tenant_id,w.message_id
            FOR UPDATE OF w SKIP LOCKED
            LIMIT 1
            "#,
            execution.as_slice(),tenant,message
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        let Some(c) = candidate else {
            tx.commit().await.map_err(classify_database_error)?;
            return Ok(None);
        };
        sqlx::query!(
            r#"
            UPDATE ingestion.replay_work
            SET generation=generation+1,lease_until_ms=floor(extract(epoch FROM clock_timestamp())*1000)::bigint+30000
            WHERE tenant_id=$1
            AND message_id=$2
            "#,
            &c.tenant_id,
            &c.message_id
        )
        .execute(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        let row = WorkRow::find(&mut tx, &c.tenant_id, &c.message_id)
            .await?
            .ok_or(PostgresReplayError::Invalid)?;
        let generation = row.generation;
        let work = row.into_work()?;
        tx.commit().await.map_err(classify_database_error)?;
        Ok(Some(ReplayLease { work, generation }))
    }
}

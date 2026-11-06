//! Atomic publication and reservation release under tenant/policy/attempt fencing.
use super::{ArchiveLease, ArchiveObject, PostgresArchiveWork};
impl PostgresArchiveWork {
    pub(super) async fn finish(
        &self,
        lease: &ArchiveLease,
        object: Option<ArchiveObject>,
    ) -> Result<bool, sqlx::Error> {
        let mut tx = self.0.begin().await?;
        sqlx::query("SET LOCAL statement_timeout='2s'")
            .execute(&mut *tx)
            .await?;
        let active: bool = sqlx::query_scalar(
            "SELECT is_active FROM metadata.tenants WHERE tenant_id=$1 FOR SHARE",
        )
        .bind(lease.tenant.as_str())
        .fetch_one(&mut *tx)
        .await?;
        if !active {
            return Ok(false);
        }
        let valid:Option<i64>=sqlx::query_scalar("SELECT j.job_id FROM history.archive_jobs j JOIN history.work_reservations r ON r.tenant_id=j.tenant_id AND r.reservation_id=j.reservation_id WHERE j.tenant_id=$1 AND j.job_id=$2 AND j.generation=$3 AND j.reservation_id=$4 AND j.object_sha256 IS NULL AND j.lease_until>clock_timestamp() AND r.expires_at>clock_timestamp() AND r.policy_revision=(SELECT max(revision) FROM history.lifecycle_policies WHERE tenant_id=j.tenant_id) FOR UPDATE OF j").bind(lease.tenant.as_str()).bind(lease.job_id).bind(lease.generation).bind(lease.reservation).fetch_optional(&mut *tx).await?;
        if valid.is_none() {
            if let Some(object) = object {
                let exact:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM history.archive_jobs WHERE tenant_id=$1 AND job_id=$2 AND generation=$3 AND object_sha256=$4 AND object_bytes=$5)").bind(lease.tenant.as_str()).bind(lease.job_id).bind(lease.generation).bind(object.digest().as_slice()).bind(object.bytes()).fetch_one(&mut *tx).await?;
                return Ok(exact);
            }
            return Ok(false);
        }
        if let Some(object) = object {
            sqlx::query("UPDATE history.archive_jobs SET object_sha256=$3,object_bytes=$4 WHERE tenant_id=$1 AND job_id=$2").bind(lease.tenant.as_str()).bind(lease.job_id).bind(object.digest().as_slice()).bind(object.bytes()).execute(&mut *tx).await?;
        } else {
            sqlx::query("UPDATE history.archive_jobs SET lease_until=clock_timestamp() WHERE tenant_id=$1 AND job_id=$2").bind(lease.tenant.as_str()).bind(lease.job_id).execute(&mut *tx).await?;
        }
        sqlx::query(
            "DELETE FROM history.work_reservations WHERE tenant_id=$1 AND reservation_id=$2",
        )
        .bind(lease.tenant.as_str())
        .bind(lease.reservation)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }
}

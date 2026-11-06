//! Trusted tenant admission; policy append and reservation share tenant-row locking.
use fleetiq_domain::TenantId;
use fleetiq_history::lifecycle::BudgetAdmission;
use sqlx::{PgPool, Row};
/// Durable short-work admission; callers own authorization and execution deadlines.
pub struct PostgresHistoryBudgets(PgPool);
impl PostgresHistoryBudgets {
    /// Uses a fully migrated pool without starting work.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    /// Reserves one slot and positive response/work bytes for at most thirty seconds.
    /// HTTP work retains its five-second deadline; explicit archive/restore turns
    /// stop within twenty seconds. Cancellation never detaches work beyond the
    /// reservation lifetime. Live restore copies also consume the byte allowance.
    /// # Errors
    /// Rejects invalid byte amounts, unknown tenants and storage failures.
    pub async fn reserve(
        &self,
        tenant: &TenantId,
        bytes: i64,
    ) -> Result<BudgetAdmission, sqlx::Error> {
        if bytes <= 0 {
            return Err(sqlx::Error::Protocol("invalid work byte allowance".into()));
        }
        let mut tx = self.0.begin().await?;
        // Waiting for the tenant lock must not preserve a pre-winner reservation snapshot.
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SET LOCAL statement_timeout = '2s'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT tenant_id FROM metadata.tenants WHERE tenant_id=$1 FOR UPDATE")
            .bind(tenant.as_str())
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM history.work_reservations WHERE tenant_id=$1 AND expires_at<=clock_timestamp()").bind(tenant.as_str()).execute(&mut *tx).await?;
        let policy=sqlx::query("SELECT revision,max_jobs,max_bytes FROM history.lifecycle_policies WHERE tenant_id=$1 ORDER BY revision DESC LIMIT 1").bind(tenant.as_str()).fetch_optional(&mut *tx).await?;
        let outcome = if let Some(policy) = policy {
            if let Some(jobs) = policy.try_get::<Option<i32>, _>("max_jobs")? {
                let max_bytes: i64 = policy.try_get("max_bytes")?;
                let allowed:bool=sqlx::query_scalar("SELECT count(*) < $2::integer AND COALESCE(sum(reserved_bytes::numeric),0)+$3::numeric+(SELECT COALESCE(sum(octet_length(payload)::numeric),0) FROM history.restore_jobs WHERE tenant_id=$1 AND status='ready' AND expires_at>clock_timestamp()) <= $4::bigint FROM history.work_reservations WHERE tenant_id=$1").bind(tenant.as_str()).bind(jobs).bind(bytes).bind(max_bytes).fetch_one(&mut *tx).await?;
                if allowed {
                    let revision: i64 = policy.try_get("revision")?;
                    let id=sqlx::query_scalar("INSERT INTO history.work_reservations (tenant_id,policy_revision,reserved_bytes) VALUES ($1,$2,$3) RETURNING reservation_id").bind(tenant.as_str()).bind(revision).bind(bytes).fetch_one(&mut *tx).await?;
                    BudgetAdmission::Reserved(id)
                } else {
                    BudgetAdmission::Denied
                }
            } else {
                BudgetAdmission::Disabled
            }
        } else {
            BudgetAdmission::Disabled
        };
        tx.commit().await?;
        Ok(outcome)
    }
    /// Releases only this tenant's reservation; repeated/expired releases are harmless.
    /// # Errors
    /// Returns storage failures; expiry remains a conservative recovery path.
    pub async fn release(&self, tenant: &TenantId, id: i64) -> Result<(), sqlx::Error> {
        let mut tx = self.0.begin().await?;
        sqlx::query("SET LOCAL statement_timeout = '2s'")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "DELETE FROM history.work_reservations WHERE tenant_id=$1 AND reservation_id=$2",
        )
        .bind(tenant.as_str())
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }
}

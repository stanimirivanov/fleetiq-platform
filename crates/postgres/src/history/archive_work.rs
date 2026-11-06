//! Durable frozen archive selections; PostgreSQL owns publication, never object uploads.
use super::{HistoryRow, PostgresHistoryBudgets};
use fleetiq_domain::TenantId;
use fleetiq_history::{
    archive_work::{
        ArchiveClaim, ArchiveFailure, ArchiveLease, ArchiveManifest, ArchiveObject,
        ArchiveWorkRepository,
    },
    lifecycle::BudgetAdmission,
};
use sqlx::{PgPool, Row};
mod completion;
/// Trusted archive scheduling/publication adapter. Constructing it starts no work.
pub struct PostgresArchiveWork(pub(super) PgPool);
impl PostgresArchiveWork {
    /// Uses an already migrated pool; callers establish operator authority.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    async fn claim_inner(
        &self,
        tenant: &TenantId,
        reservation: i64,
    ) -> Result<ArchiveClaim, sqlx::Error> {
        let mut tx = self.0.begin().await?;
        sqlx::query("SET LOCAL statement_timeout='2s'")
            .execute(&mut *tx)
            .await?;
        let enabled:bool=sqlx::query_scalar("SELECT is_active AND EXISTS (SELECT 1 FROM history.work_reservations r WHERE r.tenant_id=t.tenant_id AND r.reservation_id=$2 AND r.expires_at>clock_timestamp() AND r.policy_revision=(SELECT max(revision) FROM history.lifecycle_policies WHERE tenant_id=t.tenant_id)) FROM metadata.tenants t WHERE tenant_id=$1 FOR UPDATE").bind(tenant.as_str()).bind(reservation).fetch_one(&mut *tx).await?;
        if !enabled {
            return Ok(ArchiveClaim::Disabled);
        }
        let pending:Option<i64>=sqlx::query_scalar("SELECT job_id FROM history.archive_jobs WHERE tenant_id=$1 AND object_sha256 IS NULL AND (lease_until IS NULL OR lease_until<=clock_timestamp()) ORDER BY job_id FOR UPDATE SKIP LOCKED LIMIT 1").bind(tenant.as_str()).fetch_optional(&mut *tx).await?;
        let job_id = if let Some(id) = pending {
            id
        } else {
            let id: i64 = sqlx::query_scalar(
                "INSERT INTO history.archive_jobs (tenant_id) VALUES ($1) RETURNING job_id",
            )
            .bind(tenant.as_str())
            .fetch_one(&mut *tx)
            .await?;
            let selected = sqlx::query(include_str!("archive_work/discover.sql"))
                .bind(tenant.as_str())
                .bind(id)
                .execute(&mut *tx)
                .await?;
            if selected.rows_affected() == 0 {
                return Ok(ArchiveClaim::Idle);
            }
            id
        };
        let generation=sqlx::query_scalar("UPDATE history.archive_jobs SET generation=generation+1,lease_until=clock_timestamp()+interval '30 seconds',reservation_id=$3 WHERE tenant_id=$1 AND job_id=$2 RETURNING generation").bind(tenant.as_str()).bind(job_id).bind(reservation).fetch_one(&mut *tx).await?;
        let rows = sqlx::query_file_as!(
            HistoryRow,
            "src/history/archive_work/selection.sql",
            tenant.as_str(),
            job_id
        )
        .fetch_all(&mut *tx)
        .await?;
        let changes = rows
            .into_iter()
            .map(|r| r.into_domain(tenant))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| sqlx::Error::Protocol("invalid archive evidence".into()))?;
        if changes.is_empty() {
            return Err(sqlx::Error::Protocol("empty archive selection".into()));
        }
        tx.commit().await?;
        Ok(ArchiveClaim::Claimed(ArchiveLease {
            tenant: tenant.clone(),
            job_id,
            generation,
            reservation,
            changes,
        }))
    }
    /// Reads only verified committed manifests; pending objects are not queryable coverage.
    /// # Errors
    /// Returns a redacted failure for unavailable or invalid persisted state.
    pub async fn manifest(
        &self,
        tenant: &TenantId,
        job_id: i64,
    ) -> Result<Option<ArchiveManifest>, ArchiveFailure> {
        let mut tx = self.0.begin().await.map_err(|_| ArchiveFailure)?;
        sqlx::query("SET LOCAL statement_timeout='2s'")
            .execute(&mut *tx)
            .await
            .map_err(|_| ArchiveFailure)?;
        let row=sqlx::query("SELECT generation,object_sha256,object_bytes,(SELECT count(*) FROM history.archive_members m WHERE m.tenant_id=j.tenant_id AND m.job_id=j.job_id) members FROM history.archive_jobs j WHERE tenant_id=$1 AND job_id=$2 AND object_sha256 IS NOT NULL").bind(tenant.as_str()).bind(job_id).fetch_optional(&mut *tx).await.map_err(|_|ArchiveFailure)?;
        tx.commit().await.map_err(|_| ArchiveFailure)?;
        row.map(|r| {
            let digest: Vec<u8> = r.try_get("object_sha256").map_err(|_| ArchiveFailure)?;
            Ok(ArchiveManifest {
                job_id,
                generation: r.try_get("generation").map_err(|_| ArchiveFailure)?,
                object: ArchiveObject::new(
                    digest.try_into().map_err(|_| ArchiveFailure)?,
                    r.try_get("object_bytes").map_err(|_| ArchiveFailure)?,
                )?,
                members: r.try_get("members").map_err(|_| ArchiveFailure)?,
            })
        })
        .transpose()
    }
}
impl ArchiveWorkRepository for PostgresArchiveWork {
    async fn claim(&self, tenant: &TenantId) -> Result<ArchiveClaim, ArchiveFailure> {
        let budgets = PostgresHistoryBudgets::new(self.0.clone());
        let reservation = match budgets
            .reserve(tenant, 2_097_152)
            .await
            .map_err(|_| ArchiveFailure)?
        {
            BudgetAdmission::Disabled => return Ok(ArchiveClaim::Disabled),
            BudgetAdmission::Denied => return Ok(ArchiveClaim::Denied),
            BudgetAdmission::Reserved(id) => id,
        };
        let result = self.claim_inner(tenant, reservation).await;
        if !matches!(result, Ok(ArchiveClaim::Claimed(_))) {
            budgets
                .release(tenant, reservation)
                .await
                .map_err(|_| ArchiveFailure)?;
        }
        result.map_err(|_| ArchiveFailure)
    }
    async fn complete(
        &self,
        lease: &ArchiveLease,
        object: ArchiveObject,
    ) -> Result<bool, ArchiveFailure> {
        self.finish(lease, Some(object))
            .await
            .map_err(|_| ArchiveFailure)
    }
    async fn abandon(&self, lease: &ArchiveLease) -> Result<(), ArchiveFailure> {
        self.finish(lease, None)
            .await
            .map(|_| ())
            .map_err(|_| ArchiveFailure)
    }
}

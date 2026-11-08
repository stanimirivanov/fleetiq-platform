//! Frozen bounded export selections and temporary artifacts under shared tenant admission.
use super::PostgresHistoryBudgets;
use fleetiq_domain::TenantId;
use fleetiq_history::exports::{
    EXPORT_BYTES, ExportCodec, ExportFailure, ExportKey, ExportRequest, ExportStatus, ExportTicket,
};
use sqlx::{PgPool, Postgres, Row, Transaction};
mod authorized;
mod capture;
mod describe;
mod work;
/// Trusted export adapter; callers establish tenant authority and bounded execution.
pub struct PostgresExports(PgPool);
impl PostgresExports {
    /// Uses a migrated pool; creates no jobs or scheduler.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    async fn transaction(
        &self,
        tenant: &TenantId,
    ) -> Result<Transaction<'_, Postgres>, sqlx::Error> {
        let mut tx = self.0.begin().await?;
        sqlx::raw_sql("SET TRANSACTION ISOLATION LEVEL READ COMMITTED; SET LOCAL statement_timeout='2s'; SET LOCAL lock_timeout='2s';").execute(&mut *tx).await?;
        sqlx::query("SELECT tenant_id FROM metadata.tenants WHERE tenant_id=$1 FOR UPDATE")
            .bind(tenant.as_str())
            .fetch_one(&mut *tx)
            .await?;
        Ok(tx)
    }
    /// Freezes one exact request. A repeated key returns its original selection, even after termination.
    /// # Errors
    /// Rejects scope conflicts, missing coverage, overflow and disabled/denied current admission.
    pub async fn request<C: ExportCodec>(
        &self,
        key: &ExportKey,
        request: &ExportRequest,
        codec: &C,
    ) -> Result<ExportTicket, ExportFailure> {
        self.expire(request.tenant()).await?;
        if let Some(ticket) = self.existing(key, request).await? {
            return Ok(ticket);
        }
        let budgets = PostgresHistoryBudgets::new(self.0.clone());
        let reservation = match budgets
            .reserve(request.tenant(), EXPORT_BYTES)
            .await
            .map_err(|_| ExportFailure::Storage)?
        {
            fleetiq_history::lifecycle::BudgetAdmission::Disabled => {
                return self
                    .existing(key, request)
                    .await?
                    .ok_or(ExportFailure::Disabled);
            }
            fleetiq_history::lifecycle::BudgetAdmission::Denied => {
                return self
                    .existing(key, request)
                    .await?
                    .ok_or(ExportFailure::Denied);
            }
            fleetiq_history::lifecycle::BudgetAdmission::Reserved(id) => id,
        };
        let result = async {
            let (rows, payload) = self.capture(request, reservation, codec).await?;
            self.persist(key, request, reservation, rows, &payload)
                .await
        }
        .await;
        budgets
            .release(request.tenant(), reservation)
            .await
            .map_err(|_| ExportFailure::Storage)?;
        if result.is_err()
            && let Some(ticket) = self.existing(key, request).await?
        {
            return Ok(ticket);
        }
        result
    }
    async fn existing(
        &self,
        key: &ExportKey,
        req: &ExportRequest,
    ) -> Result<Option<ExportTicket>, ExportFailure> {
        let mut connection = self.0.acquire().await.map_err(|_| ExportFailure::Storage)?;
        Self::existing_on(key, req, &mut connection).await
    }
    async fn existing_on(
        key: &ExportKey,
        req: &ExportRequest,
        connection: &mut sqlx::PgConnection,
    ) -> Result<Option<ExportTicket>, ExportFailure> {
        let q = req.query();
        let t = q.target();
        let restore = req.restore();
        let row=sqlx::query("SELECT j.job_id,t.is_active,(j.asset_id=$3 AND j.property_id=$4 AND j.property_version=$5 AND j.start_ms=$6 AND j.end_ms=$7 AND j.manifest_id IS NOT DISTINCT FROM $8 AND j.restore_generation IS NOT DISTINCT FROM $9) AS matches FROM history.export_jobs j JOIN metadata.tenants t USING(tenant_id) WHERE j.tenant_id=$1 AND j.request_key=$2")
   .bind(req.tenant().as_str()).bind(key.as_str()).bind(t.asset_id().as_str()).bind(t.property().id().as_str()).bind(i64::from(t.property().version().get())).bind(q.start().get()).bind(q.end().get()).bind(restore.map(fleetiq_history::restore::RestoreTicket::manifest)).bind(restore.map(fleetiq_history::restore::RestoreTicket::generation)).fetch_optional(&mut *connection).await.map_err(|_|ExportFailure::Storage)?;
        row.map(|r| {
            if !r
                .try_get::<bool, _>("is_active")
                .map_err(|_| ExportFailure::Storage)?
            {
                return Err(ExportFailure::Disabled);
            }
            if !r
                .try_get::<bool, _>("matches")
                .map_err(|_| ExportFailure::Storage)?
            {
                return Err(ExportFailure::Conflict);
            }
            ExportTicket::new(r.try_get("job_id").map_err(|_| ExportFailure::Storage)?)
        })
        .transpose()
    }
    async fn persist(
        &self,
        key: &ExportKey,
        req: &ExportRequest,
        reservation: i64,
        rows: i32,
        payload: &[u8],
    ) -> Result<ExportTicket, ExportFailure> {
        let mut tx = self
            .transaction(req.tenant())
            .await
            .map_err(|_| ExportFailure::Storage)?;
        // Recheck after the tenant lock: another identical request may have committed during capture.
        if let Some(ticket) = Self::existing_on(key, req, &mut tx).await? {
            return Ok(ticket);
        }
        let q = req.query();
        let target = q.target();
        let ticket = req.restore();
        let id:i64=sqlx::query_scalar("INSERT INTO history.export_jobs(tenant_id,request_key,asset_id,property_id,property_version,start_ms,end_ms,manifest_id,restore_generation,row_count,payload,fingerprint,file_bytes,admitted_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,sha256($11::bytea),octet_length($11::bytea),$12) RETURNING job_id")
   .bind(req.tenant().as_str()).bind(key.as_str()).bind(target.asset_id().as_str()).bind(target.property().id().as_str()).bind(i64::from(target.property().version().get())).bind(q.start().get()).bind(q.end().get()).bind(ticket.map(fleetiq_history::restore::RestoreTicket::manifest)).bind(ticket.map(fleetiq_history::restore::RestoreTicket::generation)).bind(rows).bind(payload).bind(reservation).fetch_one(&mut *tx).await.map_err(|_|ExportFailure::Storage)?;
        sqlx::query(
            "DELETE FROM history.work_reservations WHERE tenant_id=$1 AND reservation_id=$2",
        )
        .bind(req.tenant().as_str())
        .bind(reservation)
        .execute(&mut *tx)
        .await
        .map_err(|_| ExportFailure::Storage)?;
        tx.commit().await.map_err(|_| ExportFailure::Storage)?;
        ExportTicket::new(id)
    }
    /// Returns availability only; expiry never becomes successful empty evidence.
    /// # Errors
    /// Returns redacted storage failures.
    pub async fn status(
        &self,
        tenant: &TenantId,
        ticket: ExportTicket,
    ) -> Result<ExportStatus, ExportFailure> {
        let state:Option<String>=sqlx::query_scalar("SELECT CASE WHEN expires_at<=clock_timestamp() THEN 'expired' ELSE status END FROM history.export_jobs j JOIN metadata.tenants t USING(tenant_id) WHERE j.tenant_id=$1 AND job_id=$2 AND t.is_active").bind(tenant.as_str()).bind(ticket.id()).fetch_optional(&self.0).await.map_err(|_|ExportFailure::Storage)?;
        Ok(match state.as_deref() {
            None => ExportStatus::Missing,
            Some("pending") => ExportStatus::Pending,
            Some("ready") => ExportStatus::Ready,
            Some("cancelled") => ExportStatus::Cancelled,
            _ => ExportStatus::Expired,
        })
    }
    /// Reads the exact ready artifact only while tenant/request remain available.
    /// # Errors
    /// Returns redacted storage/integrity failures; no object-store or hot fallback.
    pub async fn artifact(
        &self,
        tenant: &TenantId,
        ticket: ExportTicket,
    ) -> Result<Option<fleetiq_history::exports::ExportArtifact>, ExportFailure> {
        let mut tx = self
            .transaction(tenant)
            .await
            .map_err(|_| ExportFailure::Storage)?;
        let row=sqlx::query!("SELECT payload AS \"payload!\",floor(extract(epoch FROM expires_at)*1000)::bigint AS \"expires!\",manifest_id,restore_generation FROM history.export_jobs j JOIN metadata.tenants t USING(tenant_id) WHERE j.tenant_id=$1 AND job_id=$2 AND t.is_active AND status='ready' AND expires_at>clock_timestamp() AND octet_length(payload)=file_bytes AND sha256(payload)=fingerprint",tenant.as_str(),ticket.id()).fetch_optional(&mut *tx).await.map_err(|_|ExportFailure::Storage)?;
        tx.commit().await.map_err(|_| ExportFailure::Storage)?;
        row.map(|r| {
            let restore = match (r.manifest_id, r.restore_generation) {
                (None, None) => None,
                (Some(m), Some(g)) => Some(
                    fleetiq_history::restore::RestoreTicket::new(m, g)
                        .map_err(|_| ExportFailure::Coverage)?,
                ),
                _ => return Err(ExportFailure::Coverage),
            };
            fleetiq_history::exports::ExportArtifact::new(r.payload, r.expires, restore)
        })
        .transpose()
    }

    /// Cancels this identity without reopening it or prematurely releasing running work.
    /// # Errors
    /// Returns redacted storage failures.
    pub async fn cancel(
        &self,
        tenant: &TenantId,
        ticket: ExportTicket,
    ) -> Result<(), ExportFailure> {
        let mut tx = self
            .transaction(tenant)
            .await
            .map_err(|_| ExportFailure::Storage)?;
        sqlx::query("UPDATE history.export_jobs SET status='cancelled',payload=NULL WHERE tenant_id=$1 AND job_id=$2 AND status IN ('pending','ready')").bind(tenant.as_str()).bind(ticket.id()).execute(&mut *tx).await.map_err(|_|ExportFailure::Storage)?;
        tx.commit().await.map_err(|_| ExportFailure::Storage)
    }
    /// Reconciles at most sixteen expired copies, preserving identity and running-attempt fences.
    /// # Errors
    /// Returns redacted storage failures; unreconciled expired copies are already unreadable.
    pub async fn expire(&self, tenant: &TenantId) -> Result<(), ExportFailure> {
        self.expire_counted(tenant).await.map(|_| ())
    }

    /// Reports committed counts of expired export jobs and cleared stale leases.
    /// Each count is bounded to sixteen. Live attempts retain their fences.
    /// # Errors
    /// Returns redacted storage failures.
    pub async fn expire_counted(&self, tenant: &TenantId) -> Result<(u64, u64), ExportFailure> {
        let mut tx = self
            .transaction(tenant)
            .await
            .map_err(|_| ExportFailure::Storage)?;
        let expired = sqlx::query("UPDATE history.export_jobs SET status='expired',payload=NULL WHERE tenant_id=$1 AND job_id IN (SELECT job_id FROM history.export_jobs WHERE tenant_id=$1 AND expires_at<=statement_timestamp() AND status IN ('pending','ready') ORDER BY expires_at,job_id LIMIT 16)").bind(tenant.as_str()).execute(&mut *tx).await.map_err(|_|ExportFailure::Storage)?;
        let leases = sqlx::query("UPDATE history.export_jobs SET lease_until=NULL WHERE tenant_id=$1 AND job_id IN (SELECT job_id FROM history.export_jobs WHERE tenant_id=$1 AND lease_until<=statement_timestamp() ORDER BY lease_until,job_id LIMIT 16)").bind(tenant.as_str()).execute(&mut *tx).await.map_err(|_|ExportFailure::Storage)?;
        tx.commit().await.map_err(|_| ExportFailure::Storage)?;
        Ok((expired.rows_affected(), leases.rows_affected()))
    }
}

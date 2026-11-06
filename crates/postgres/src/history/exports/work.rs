//! Exact attempts share the already-held job slot; stale work cannot reopen artifacts.
use super::{ExportFailure, ExportTicket, PostgresExports};
use fleetiq_domain::TenantId;
use fleetiq_history::exports::{ExportClaim, ExportLease, ExportRepository};
use sqlx::Row;
impl ExportRepository for PostgresExports {
    async fn claim(
        &self,
        tenant: &TenantId,
        ticket: ExportTicket,
    ) -> Result<ExportClaim, ExportFailure> {
        self.expire(tenant).await?;
        let mut tx = self
            .transaction(tenant)
            .await
            .map_err(|_| ExportFailure::Storage)?;
        let allowed: bool = sqlx::query_scalar("SELECT history.export_admitted($1)")
            .bind(tenant.as_str())
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| ExportFailure::Storage)?;
        if !allowed {
            return Ok(ExportClaim::Denied);
        }
        let row=sqlx::query("UPDATE history.export_jobs SET attempt=attempt+1,lease_until=clock_timestamp()+interval '30 seconds' WHERE tenant_id=$1 AND job_id=$2 AND status='pending' AND expires_at>clock_timestamp()+interval '20 seconds' AND (lease_until IS NULL OR lease_until<=clock_timestamp()) RETURNING attempt,payload").bind(tenant.as_str()).bind(ticket.id()).fetch_optional(&mut *tx).await.map_err(|_|ExportFailure::Storage)?;
        let claim = row
            .map(|r| {
                Ok(ExportLease {
                    tenant: tenant.clone(),
                    ticket,
                    attempt: r.try_get("attempt").map_err(|_| ExportFailure::Storage)?,
                    payload: r.try_get("payload").map_err(|_| ExportFailure::Storage)?,
                })
            })
            .transpose()?;
        tx.commit().await.map_err(|_| ExportFailure::Storage)?;
        Ok(claim.map_or(ExportClaim::Idle, ExportClaim::Claimed))
    }
    async fn complete(&self, lease: &ExportLease, bytes: &[u8]) -> Result<bool, ExportFailure> {
        if bytes.is_empty() || bytes.len() > 2_097_152 {
            return Ok(false);
        }
        let mut tx = self
            .transaction(&lease.tenant)
            .await
            .map_err(|_| ExportFailure::Storage)?;
        let allowed: bool = sqlx::query_scalar("SELECT history.export_admitted($1)")
            .bind(lease.tenant.as_str())
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| ExportFailure::Storage)?;
        if !allowed {
            return Ok(false);
        }
        let changed=sqlx::query("UPDATE history.export_jobs SET status='ready',lease_until=NULL WHERE tenant_id=$1 AND job_id=$2 AND attempt=$3 AND status='pending' AND lease_until>clock_timestamp() AND expires_at>clock_timestamp() AND payload=$4 AND fingerprint=sha256($4::bytea)").bind(lease.tenant.as_str()).bind(lease.ticket.id()).bind(lease.attempt).bind(bytes).execute(&mut *tx).await.map_err(|_|ExportFailure::Storage)?.rows_affected()>0;
        let exact = if changed {
            true
        } else {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM history.export_jobs WHERE tenant_id=$1 AND job_id=$2 AND attempt=$3 AND status='ready' AND expires_at>clock_timestamp() AND payload=$4)").bind(lease.tenant.as_str()).bind(lease.ticket.id()).bind(lease.attempt).bind(bytes).fetch_one(&mut *tx).await.map_err(|_|ExportFailure::Storage)?
        };
        tx.commit().await.map_err(|_| ExportFailure::Storage)?;
        Ok(exact)
    }
    async fn abandon(&self, lease: &ExportLease) -> Result<(), ExportFailure> {
        let mut tx = self
            .transaction(&lease.tenant)
            .await
            .map_err(|_| ExportFailure::Storage)?;
        sqlx::query("UPDATE history.export_jobs SET lease_until=NULL WHERE tenant_id=$1 AND job_id=$2 AND attempt=$3 AND lease_until IS NOT NULL").bind(lease.tenant.as_str()).bind(lease.ticket.id()).bind(lease.attempt).execute(&mut *tx).await.map_err(|_|ExportFailure::Storage)?;
        tx.commit().await.map_err(|_| ExportFailure::Storage)
    }
}

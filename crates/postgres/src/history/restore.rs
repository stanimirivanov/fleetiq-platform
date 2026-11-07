//! Temporary verified-file staging, independently expiring from immutable archive evidence.
use fleetiq_access::{AuthorizedTenant, ManageOperations, ReadTelemetry};
use fleetiq_domain::TenantId;
use fleetiq_history::restore::{RestoreFailure, RestoreStatus, RestoreTicket};
use sqlx::{PgPool, Postgres, Transaction};
mod history_snapshot;
mod work;
/// Safe admission outcomes for an authorized restore request.
#[derive(Debug)]
pub enum RestoreRequestError {
    /// The expected generation is stale or the current restore is still active.
    Conflict,
    /// The manifest or lifecycle policy does not permit this request.
    Unavailable,
    /// The durable request could not be confirmed.
    Storage,
}

/// Trusted restore request/staging adapter; construction starts no work.
pub struct PostgresRestore(pub(super) PgPool);
impl PostgresRestore {
    /// Uses an already migrated pool; callers establish authority for every operation.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    async fn transaction(&self) -> Result<Transaction<'_, Postgres>, sqlx::Error> {
        let mut tx = self.0.begin().await?;
        sqlx::query("SET LOCAL statement_timeout='2s'")
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }
    /// Creates a one-hour request from a committed manifest. Exact retries preserve TTL/state.
    /// Use expected=0 initially; a new request after cancellation/expiry names the prior generation.
    /// # Errors
    /// Rejects stale/invalid requests, unpublished manifests, disabled tenants/policies and storage failures.
    pub async fn request(
        &self,
        tenant: &TenantId,
        manifest: i64,
        expected: i64,
    ) -> Result<RestoreTicket, RestoreFailure> {
        self.request_outcome(tenant, manifest, expected)
            .await
            .map_err(|_| RestoreFailure)
    }

    /// Requests a manifest within independent grants held by one actor.
    /// # Errors
    /// Rejects mismatched authority, unavailable manifests and stale generations.
    pub async fn request_authorized(
        &self,
        read: AuthorizedTenant<ReadTelemetry>,
        manage: AuthorizedTenant<ManageOperations>,
        manifest: i64,
        expected: i64,
    ) -> Result<RestoreTicket, RestoreRequestError> {
        if read.tenant_id() != manage.tenant_id() || read.actor_id() != manage.actor_id() {
            return Err(RestoreRequestError::Unavailable);
        }
        self.request_outcome(read.tenant_id(), manifest, expected)
            .await
    }

    async fn request_outcome(
        &self,
        tenant: &TenantId,
        manifest: i64,
        expected: i64,
    ) -> Result<RestoreTicket, RestoreRequestError> {
        if manifest <= 0 || expected < 0 || expected == i64::MAX {
            return Err(RestoreRequestError::Conflict);
        }
        let mut tx = self
            .transaction()
            .await
            .map_err(|_| RestoreRequestError::Storage)?;
        if !lock(&mut tx, tenant)
            .await
            .map_err(|_| RestoreRequestError::Storage)?
        {
            return Err(RestoreRequestError::Unavailable);
        }
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM history.archive_jobs WHERE tenant_id=$1 AND job_id=$2 AND object_sha256 IS NOT NULL) AND COALESCE((SELECT max_jobs IS NOT NULL FROM history.lifecycle_policies WHERE tenant_id=$1 ORDER BY revision DESC LIMIT 1),false)").bind(tenant.as_str()).bind(manifest).fetch_one(&mut *tx).await.map_err(|_|RestoreRequestError::Storage)?;
        if !valid {
            return Err(RestoreRequestError::Unavailable);
        }
        let next:Option<i64>=if expected==0 {
            sqlx::query_scalar("INSERT INTO history.restore_jobs (tenant_id,manifest_id,request_generation) VALUES ($1,$2,1) ON CONFLICT DO NOTHING RETURNING request_generation").bind(tenant.as_str()).bind(manifest).fetch_optional(&mut *tx).await
        } else {
            sqlx::query_scalar("UPDATE history.restore_jobs SET request_generation=request_generation+1,status='pending',payload=NULL,lease_until=NULL,reservation_id=NULL,expires_at=clock_timestamp()+interval '1 hour' WHERE tenant_id=$1 AND manifest_id=$2 AND request_generation=$3 AND (status IN ('cancelled','expired') OR expires_at<=clock_timestamp()) RETURNING request_generation").bind(tenant.as_str()).bind(manifest).bind(expected).fetch_optional(&mut *tx).await
        }.map_err(|_|RestoreRequestError::Storage)?;
        let generation = if let Some(next) = next {
            next
        } else {
            sqlx::query_scalar("SELECT request_generation FROM history.restore_jobs WHERE tenant_id=$1 AND manifest_id=$2 AND request_generation=$3").bind(tenant.as_str()).bind(manifest).bind(expected+1).fetch_optional(&mut *tx).await.map_err(|_|RestoreRequestError::Storage)?.ok_or(RestoreRequestError::Conflict)?
        };
        tx.commit()
            .await
            .map_err(|_| RestoreRequestError::Storage)?;
        RestoreTicket::new(manifest, generation).map_err(|_| RestoreRequestError::Storage)
    }
    /// Reports current generation-bound availability using the database clock.
    /// # Errors
    /// Returns redacted storage or invalid-state failures.
    pub async fn status(
        &self,
        tenant: &TenantId,
        ticket: &RestoreTicket,
    ) -> Result<RestoreStatus, RestoreFailure> {
        let mut tx = self.transaction().await.map_err(|_| RestoreFailure)?;
        let status:Option<String>=sqlx::query_scalar("SELECT CASE WHEN expires_at<=clock_timestamp() THEN 'expired' ELSE status END FROM history.restore_jobs j JOIN metadata.tenants t USING (tenant_id) WHERE j.tenant_id=$1 AND manifest_id=$2 AND request_generation=$3 AND t.is_active").bind(tenant.as_str()).bind(ticket.manifest()).bind(ticket.generation()).fetch_optional(&mut *tx).await.map_err(|_|RestoreFailure)?;
        match status.as_deref() {
            None => Ok(RestoreStatus::Missing),
            Some("pending") => Ok(RestoreStatus::Pending),
            Some("ready") => Ok(RestoreStatus::Ready),
            Some("cancelled") => Ok(RestoreStatus::Cancelled),
            Some("expired") => Ok(RestoreStatus::Expired),
            _ => Err(RestoreFailure),
        }
    }
    /// Reads at most one verified file for the exact live request; never falls back to hot history.
    /// # Errors
    /// Returns redacted storage failures. Callers decode with the bounded archive codec.
    pub async fn staged(
        &self,
        tenant: &TenantId,
        ticket: &RestoreTicket,
    ) -> Result<Option<Vec<u8>>, RestoreFailure> {
        let mut tx = self.transaction().await.map_err(|_| RestoreFailure)?;
        sqlx::query_scalar("SELECT payload FROM history.restore_jobs j JOIN metadata.tenants t USING (tenant_id) WHERE j.tenant_id=$1 AND manifest_id=$2 AND request_generation=$3 AND status='ready' AND expires_at>clock_timestamp() AND t.is_active").bind(tenant.as_str()).bind(ticket.manifest()).bind(ticket.generation()).fetch_optional(&mut *tx).await.map_err(|_|RestoreFailure)
    }
    /// Cancels only this request generation and removes its temporary copy.
    /// In-flight work retains its reservation until it stops or its lease expires.
    /// # Errors
    /// Returns redacted storage failures; repeated current-generation cancellation is harmless.
    pub async fn cancel(
        &self,
        tenant: &TenantId,
        ticket: &RestoreTicket,
    ) -> Result<bool, RestoreFailure> {
        let mut tx = self.transaction().await.map_err(|_| RestoreFailure)?;
        lock(&mut tx, tenant).await.map_err(|_| RestoreFailure)?;
        let changed=sqlx::query("UPDATE history.restore_jobs SET status='cancelled',payload=NULL WHERE tenant_id=$1 AND manifest_id=$2 AND request_generation=$3 AND expires_at>clock_timestamp() AND status IN ('pending','ready','cancelled')").bind(tenant.as_str()).bind(ticket.manifest()).bind(ticket.generation()).execute(&mut *tx).await.map_err(|_|RestoreFailure)?.rows_affected()>0;
        tx.commit().await.map_err(|_| RestoreFailure)?;
        Ok(changed)
    }
    /// Removes at most sixteen expired temporary copies/requests; archives remain untouched.
    /// # Errors
    /// Returns redacted storage failures. Uncollected expired copies are already unreadable.
    pub async fn expire(&self, tenant: &TenantId) -> Result<(), RestoreFailure> {
        let mut tx = self.transaction().await.map_err(|_| RestoreFailure)?;
        lock(&mut tx, tenant).await.map_err(|_| RestoreFailure)?;
        cleanup(&mut tx, tenant).await.map_err(|_| RestoreFailure)?;
        tx.commit().await.map_err(|_| RestoreFailure)
    }
}
async fn lock(tx: &mut Transaction<'_, Postgres>, tenant: &TenantId) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT is_active FROM metadata.tenants WHERE tenant_id=$1 FOR UPDATE")
        .bind(tenant.as_str())
        .fetch_one(&mut **tx)
        .await
}
async fn cleanup(tx: &mut Transaction<'_, Postgres>, tenant: &TenantId) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE history.restore_jobs SET status='expired',payload=NULL WHERE tenant_id=$1 AND manifest_id IN (SELECT manifest_id FROM history.restore_jobs WHERE tenant_id=$1 AND expires_at<=clock_timestamp() AND status IN ('pending','ready') ORDER BY (payload IS NULL),expires_at,manifest_id FOR UPDATE SKIP LOCKED LIMIT 16)").bind(tenant.as_str()).execute(&mut **tx).await?;
    Ok(())
}

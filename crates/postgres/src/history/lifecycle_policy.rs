//! Trusted administration persistence; callers must authorize before invoking it.
use fleetiq_domain::{ActorId, TenantId};
use fleetiq_history::lifecycle::{LifecyclePolicy, LifecycleSettings};
use sqlx::{PgPool, Row};

/// Append-only tenant policy ledger. No runtime worker is enabled by this adapter.
#[derive(Clone, Debug)]
pub struct PostgresLifecyclePolicyRepository {
    pool: PgPool,
}
impl PostgresLifecyclePolicyRepository {
    /// Uses a migrated pool; does not grant caller authority.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
    /// Appends exactly the successor of the expected tenant revision (zero for absent).
    /// # Errors
    /// Rejects stale revisions, unknown tenants/actors and storage failures.
    pub async fn append(
        &self,
        tenant: &TenantId,
        actor: &ActorId,
        expected: i64,
        settings: LifecycleSettings,
    ) -> Result<(), sqlx::Error> {
        let next = expected
            .checked_add(1)
            .filter(|_| expected >= 0)
            .ok_or(sqlx::Error::Protocol("invalid lifecycle revision".into()))?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT tenant_id FROM metadata.tenants WHERE tenant_id=$1 FOR UPDATE")
            .bind(tenant.as_str())
            .fetch_one(&mut *tx)
            .await?;
        let current: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(revision),0) FROM history.lifecycle_policies WHERE tenant_id=$1",
        )
        .bind(tenant.as_str())
        .fetch_one(&mut *tx)
        .await?;
        if current != expected {
            return Err(sqlx::Error::Protocol("stale lifecycle revision".into()));
        }
        let (hot, archive, jobs, bytes) = match settings {
            LifecycleSettings::Disabled => (None, None, None, None),
            LifecycleSettings::Configured(p) => (
                Some(p.hot_days()),
                Some(p.archive_days()),
                Some(p.max_jobs()),
                Some(p.max_bytes()),
            ),
        };
        sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ($1,$2,$3,$4,$5,$6,$7)")
            .bind(tenant.as_str()).bind(next).bind(actor.as_str()).bind(hot).bind(archive).bind(jobs).bind(bytes).execute(&mut *tx).await?;
        tx.commit().await
    }
    /// Reads the latest policy, returning disabled revision zero for absent policy.
    /// # Errors
    /// Returns storage failures or invalid durable policy values.
    pub async fn latest(&self, tenant: &TenantId) -> Result<(i64, LifecycleSettings), sqlx::Error> {
        let row = sqlx::query("SELECT revision,hot_days,archive_days,max_jobs,max_bytes FROM history.lifecycle_policies WHERE tenant_id=$1 ORDER BY revision DESC LIMIT 1").bind(tenant.as_str()).fetch_optional(&self.pool).await?;
        let Some(row) = row else {
            return Ok((0, LifecycleSettings::Disabled));
        };
        let settings = match row.try_get::<Option<i32>, _>("hot_days")? {
            None => LifecycleSettings::Disabled,
            Some(hot) => LifecycleSettings::Configured(
                LifecyclePolicy::new(
                    hot,
                    row.try_get("archive_days")?,
                    row.try_get("max_jobs")?,
                    row.try_get("max_bytes")?,
                )
                .map_err(|_| sqlx::Error::Protocol("invalid stored lifecycle policy".into()))?,
            ),
        };
        Ok((row.try_get("revision")?, settings))
    }
}

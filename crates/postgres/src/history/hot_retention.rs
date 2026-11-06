//! Explicit trusted operator retention. No construction, policy or preview starts deletion.
use crate::history::{HistoryRow, PostgresHistoryBudgets};
use fleetiq_domain::TenantId;
use fleetiq_history::{
    archive_work::ArchiveObject,
    restore::{ArchiveRestoreSource, RestoredSegment},
    retention_work::{
        RetentionCheck, RetentionFailure, RetentionRepository, RetentionTarget, RetentionTick,
    },
};
use sqlx::{PgPool, Row};
/// Private database lifecycle adapter. The composition caller establishes operator authority.
pub struct PostgresHotRetention(PgPool);
impl PostgresHotRetention {
    /// Uses a migrated pool without starting work.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    /// Records explicit human-reviewed approval for exactly one current policy revision.
    /// This trusted operator function is not exposed through operations.read or HTTP.
    /// # Errors
    /// Rejects inactive actor/tenant, stale/disabled policy, or database failures.
    pub async fn approve(
        &self,
        tenant: &TenantId,
        revision: i64,
        actor: &str,
    ) -> Result<(), RetentionFailure> {
        let rows=sqlx::query("INSERT INTO history.retention_controls (tenant_id,policy_revision,actor_id) SELECT t.tenant_id,p.revision,a.actor_id FROM metadata.tenants t JOIN history.lifecycle_policies p USING (tenant_id) CROSS JOIN identity.actors a WHERE t.tenant_id=$1 AND t.is_active AND a.actor_id=$3 AND a.is_active AND p.revision=$2 AND p.hot_days IS NOT NULL AND p.revision=(SELECT max(revision) FROM history.lifecycle_policies WHERE tenant_id=$1) ON CONFLICT (tenant_id) DO UPDATE SET policy_revision=EXCLUDED.policy_revision,actor_id=EXCLUDED.actor_id")
            .bind(tenant.as_str()).bind(revision).bind(actor).execute(&self.0).await.map_err(|_|RetentionFailure)?.rows_affected();
        if rows != 1 {
            return Err(RetentionFailure);
        }
        Ok(())
    }
    /// Revokes future execution; an already committed receipt remains immutable.
    /// # Errors
    /// Returns redacted database failures.
    pub async fn revoke(&self, tenant: &TenantId) -> Result<(), RetentionFailure> {
        sqlx::query("DELETE FROM history.retention_controls WHERE tenant_id=$1")
            .bind(tenant.as_str())
            .execute(&self.0)
            .await
            .map_err(|_| RetentionFailure)?;
        Ok(())
    }
    async fn complete(
        &self,
        target: &RetentionTarget,
        segment: &RestoredSegment,
        reservation: i64,
    ) -> Result<RetentionTick, sqlx::Error> {
        let mut tx = self.0.begin().await?;
        sqlx::raw_sql("SET TRANSACTION ISOLATION LEVEL READ COMMITTED; SET LOCAL statement_timeout='2s'; SET LOCAL lock_timeout='2s';")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT history.lock_retention_dependencies()")
            .execute(&mut *tx)
            .await?;
        let done:Option<i32>=sqlx::query_scalar("SELECT records FROM history.retention_receipts WHERE tenant_id=$1 AND manifest_id=$2 AND policy_revision=$3")
            .bind(target.tenant().as_str()).bind(target.manifest()).bind(target.revision()).fetch_optional(&mut *tx).await?;
        if let Some(n) = done {
            return Ok(RetentionTick::AlreadyReclaimed(n.cast_unsigned()));
        }
        let live:Option<i64>=sqlx::query_scalar("SELECT reservation_id FROM history.work_reservations WHERE tenant_id=$1 AND reservation_id=$2 AND policy_revision=$3 AND reserved_bytes>=2097152 AND expires_at>clock_timestamp() FOR UPDATE")
            .bind(target.tenant().as_str()).bind(reservation).bind(target.revision()).fetch_optional(&mut *tx).await?;
        if live.is_none() {
            return Ok(RetentionTick::Protected);
        }
        let blocked: bool = sqlx::query_scalar("SELECT history.retention_blocked($1,$2,$3)")
            .bind(target.tenant().as_str())
            .bind(target.manifest())
            .bind(target.revision())
            .fetch_one(&mut *tx)
            .await?;
        if blocked {
            return Ok(RetentionTick::Protected);
        }
        // Fresh provider read must match the immutable manifest's WHOLE file, not its logical digest.
        let matches:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM history.archive_jobs WHERE tenant_id=$1 AND job_id=$2 AND object_sha256=sha256($3::bytea) AND object_bytes=octet_length($3::bytea))")
            .bind(target.tenant().as_str()).bind(target.manifest()).bind(segment.bytes()).fetch_one(&mut *tx).await?;
        if !matches {
            return Err(sqlx::Error::Protocol("archive verification failed".into()));
        }
        let originals = sqlx::query_file_as!(
            HistoryRow,
            "src/history/archive_work/selection.sql",
            target.tenant().as_str(),
            target.manifest()
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|r| r.into_domain(target.tenant()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| sqlx::Error::Protocol("invalid original evidence".into()))?;
        if originals.is_empty()
            || originals.len() > 16
            || originals.len() != segment.changes().len()
            || originals.iter().any(|c| !segment.changes().contains(c))
        {
            return Err(sqlx::Error::Protocol(
                "archive membership or evidence mismatch".into(),
            ));
        }
        let count = i32::try_from(originals.len())
            .map_err(|_| sqlx::Error::Protocol("selection limit".into()))?;
        sqlx::query("INSERT INTO history.retention_receipts (tenant_id,manifest_id,policy_revision,actor_id,records,reservation_id) SELECT tenant_id,$2,policy_revision,actor_id,$4,$5 FROM history.retention_controls WHERE tenant_id=$1 AND policy_revision=$3")
            .bind(target.tenant().as_str()).bind(target.manifest()).bind(target.revision()).bind(count).bind(reservation).execute(&mut *tx).await?;
        let changed=sqlx::query("UPDATE history.asset_property_changes h SET event_at_ms=NULL,resolution_effective_at_ms=NULL,resolution_known_at_ms=NULL FROM history.archive_members m WHERE m.tenant_id=$1 AND m.job_id=$2 AND (h.tenant_id,h.message_id,h.protocol_package_id,h.protocol_package_version,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis)=(m.tenant_id,m.message_id,m.protocol_package_id,m.protocol_package_version,m.output_index,m.binding_id,m.binding_revision,m.event_time_basis)")
            .bind(target.tenant().as_str()).bind(target.manifest()).execute(&mut *tx).await?.rows_affected();
        if changed != u64::from(count.cast_unsigned()) {
            return Err(sqlx::Error::Protocol("membership changed".into()));
        }
        tx.commit().await?;
        Ok(RetentionTick::Reclaimed(count.cast_unsigned()))
    }
}

impl RetentionRepository for PostgresHotRetention {
    async fn inspect(&self, target: &RetentionTarget) -> Result<RetentionCheck, RetentionFailure> {
        let mut tx = self.0.begin().await.map_err(|_| RetentionFailure)?;
        sqlx::raw_sql(
            "SET TRANSACTION ISOLATION LEVEL READ COMMITTED; SET LOCAL statement_timeout='2s';",
        )
        .execute(&mut *tx)
        .await
        .map_err(|_| RetentionFailure)?;
        let n:Option<i32>=sqlx::query_scalar("SELECT records FROM history.retention_receipts WHERE tenant_id=$1 AND manifest_id=$2 AND policy_revision=$3")
            .bind(target.tenant().as_str()).bind(target.manifest()).bind(target.revision()).fetch_optional(&mut *tx).await.map_err(|_|RetentionFailure)?;
        if let Some(n) = n {
            return Ok(RetentionCheck::Finished(RetentionTick::AlreadyReclaimed(
                u32::try_from(n).map_err(|_| RetentionFailure)?,
            )));
        }
        let row=sqlx::query("SELECT c.policy_revision,t.is_active tenant_active,a.is_active actor_active,(SELECT max(revision) FROM history.lifecycle_policies WHERE tenant_id=c.tenant_id) latest,j.object_sha256,j.object_bytes FROM history.retention_controls c JOIN metadata.tenants t USING (tenant_id) JOIN identity.actors a USING (actor_id) LEFT JOIN history.archive_jobs j ON j.tenant_id=c.tenant_id AND j.job_id=$2 WHERE c.tenant_id=$1")
            .bind(target.tenant().as_str()).bind(target.manifest()).fetch_optional(&mut *tx).await.map_err(|_|RetentionFailure)?;
        let Some(row) = row else {
            return Ok(RetentionCheck::Finished(RetentionTick::Disabled));
        };
        if row
            .try_get::<i64, _>("policy_revision")
            .map_err(|_| RetentionFailure)?
            != target.revision()
            || row
                .try_get::<Option<i64>, _>("latest")
                .map_err(|_| RetentionFailure)?
                != Some(target.revision())
            || !row
                .try_get::<bool, _>("tenant_active")
                .map_err(|_| RetentionFailure)?
            || !row
                .try_get::<bool, _>("actor_active")
                .map_err(|_| RetentionFailure)?
        {
            return Ok(RetentionCheck::Finished(RetentionTick::Disabled));
        }
        let Some(digest) = row
            .try_get::<Option<Vec<u8>>, _>("object_sha256")
            .map_err(|_| RetentionFailure)?
        else {
            return Ok(RetentionCheck::Finished(RetentionTick::Protected));
        };
        let object = ArchiveObject::new(
            digest.try_into().map_err(|_| RetentionFailure)?,
            row.try_get("object_bytes").map_err(|_| RetentionFailure)?,
        )
        .map_err(|_| RetentionFailure)?;
        tx.commit().await.map_err(|_| RetentionFailure)?;
        Ok(RetentionCheck::Verify(object))
    }
    async fn reclaim<S: ArchiveRestoreSource>(
        &self,
        target: &RetentionTarget,
        source: &S,
    ) -> Result<RetentionTick, RetentionFailure> {
        let object = match self.inspect(target).await? {
            RetentionCheck::Finished(result) => return Ok(result),
            RetentionCheck::Verify(object) => object,
        };
        let budgets = PostgresHistoryBudgets::new(self.0.clone());
        let reservation = match budgets
            .reserve(target.tenant(), 2_097_152)
            .await
            .map_err(|_| RetentionFailure)?
        {
            fleetiq_history::lifecycle::BudgetAdmission::Reserved(id) => id,
            fleetiq_history::lifecycle::BudgetAdmission::Disabled => {
                return Ok(RetentionTick::Disabled);
            }
            fleetiq_history::lifecycle::BudgetAdmission::Denied => {
                return Ok(RetentionTick::Denied);
            }
        };
        let result = async {
            let segment = source
                .load(target.tenant(), object)
                .await
                .map_err(|_| RetentionFailure)?;
            self.complete(target, &segment, reservation)
                .await
                .map_err(|_| RetentionFailure)
        }
        .await;
        budgets
            .release(target.tenant(), reservation)
            .await
            .map_err(|_| RetentionFailure)?;
        result
    }
}

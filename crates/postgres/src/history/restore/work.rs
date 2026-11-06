//! Reservation/live-copy accounting and fenced atomic staging publication.
use super::{PostgresRestore, cleanup, lock};
use crate::history::{
    PostgresArchiveWork, PostgresHistoryBudgets, property_change_log::basis_name,
};
use fleetiq_domain::TenantId;
use fleetiq_history::{
    archive_work::ArchiveObject,
    lifecycle::BudgetAdmission,
    restore::{
        RestoreClaim, RestoreFailure, RestoreLease, RestoreRepository, RestoreStatus,
        RestoreTicket, RestoredSegment,
    },
};
use serde_json::json;
impl PostgresRestore {
    async fn claim_inner(
        &self,
        tenant: &TenantId,
        ticket: RestoreTicket,
        reservation: i64,
        object: ArchiveObject,
    ) -> Result<RestoreClaim, sqlx::Error> {
        let mut tx = self.transaction().await?;
        if !lock(&mut tx, tenant).await? {
            return Ok(RestoreClaim::Disabled);
        }
        cleanup(&mut tx, tenant).await?;
        let fits = sqlx::query_file!(
            "src/history/restore/quota.sql",
            tenant.as_str(),
            reservation
        )
        .fetch_one(&mut *tx)
        .await?
        .fits;
        if !fits {
            tx.commit().await?;
            return Ok(RestoreClaim::Denied);
        }
        let attempt:Option<i64>=sqlx::query_scalar("UPDATE history.restore_jobs SET attempt=attempt+1,lease_until=clock_timestamp()+interval '30 seconds',reservation_id=$4 WHERE tenant_id=$1 AND manifest_id=$2 AND request_generation=$3 AND status='pending' AND expires_at>clock_timestamp()+interval '20 seconds' AND (lease_until IS NULL OR lease_until<=clock_timestamp()) RETURNING attempt").bind(tenant.as_str()).bind(ticket.manifest()).bind(ticket.generation()).bind(reservation).fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        Ok(attempt.map_or(RestoreClaim::Idle, |attempt| {
            RestoreClaim::Claimed(RestoreLease {
                tenant: tenant.clone(),
                ticket,
                attempt,
                reservation,
                object,
            })
        }))
    }
    async fn publish(
        &self,
        lease: &RestoreLease,
        segment: &RestoredSegment,
    ) -> Result<bool, sqlx::Error> {
        if segment
            .changes()
            .iter()
            .any(|c| c.tenant_id() != &lease.tenant)
        {
            return Ok(false);
        }
        let keys=segment.changes().iter().map(|change| {
            let id=change.id();let source=id.observation_id();let package=source.decoded_by();
            json!({"message_id":source.message_id().as_str(),"protocol_package_id":package.id().as_str(),"protocol_package_version":package.version().as_str(),"output_index":i64::from(source.output_index()),"binding_id":id.binding_id().as_str(),"binding_revision":id.binding_revision().get().to_string(),"event_time_basis":basis_name(id.event_time_basis())})
        }).collect::<Vec<_>>();
        let mut tx = self.transaction().await?;
        if !lock(&mut tx, &lease.tenant).await? {
            return Ok(false);
        }
        let keys = json!(keys);
        let changed = sqlx::query_file!(
            "src/history/restore/complete.sql",
            lease.tenant.as_str(),
            lease.ticket.manifest(),
            lease.ticket.generation(),
            lease.attempt,
            lease.reservation,
            segment.bytes(),
            keys
        )
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;
        if changed {
            sqlx::query(
                "DELETE FROM history.work_reservations WHERE tenant_id=$1 AND reservation_id=$2",
            )
            .bind(lease.tenant.as_str())
            .bind(lease.reservation)
            .execute(&mut *tx)
            .await?;
        }
        let exact = if changed {
            true
        } else {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM history.restore_jobs WHERE tenant_id=$1 AND manifest_id=$2 AND request_generation=$3 AND attempt=$4 AND status='ready' AND expires_at>clock_timestamp() AND payload=$5)").bind(lease.tenant.as_str()).bind(lease.ticket.manifest()).bind(lease.ticket.generation()).bind(lease.attempt).bind(segment.bytes()).fetch_one(&mut *tx).await?
        };
        tx.commit().await?;
        Ok(exact)
    }
}
impl RestoreRepository for PostgresRestore {
    async fn claim(
        &self,
        tenant: &TenantId,
        ticket: RestoreTicket,
    ) -> Result<RestoreClaim, RestoreFailure> {
        if self.status(tenant, &ticket).await? != RestoreStatus::Pending {
            return Ok(RestoreClaim::Idle);
        }
        let Some(manifest) = PostgresArchiveWork::new(self.0.clone())
            .manifest(tenant, ticket.manifest())
            .await
            .map_err(|_| RestoreFailure)?
        else {
            return Ok(RestoreClaim::Idle);
        };
        let budget = PostgresHistoryBudgets::new(self.0.clone());
        let reservation = match budget
            .reserve(tenant, manifest.object.bytes())
            .await
            .map_err(|_| RestoreFailure)?
        {
            BudgetAdmission::Disabled => return Ok(RestoreClaim::Disabled),
            BudgetAdmission::Denied => return Ok(RestoreClaim::Denied),
            BudgetAdmission::Reserved(id) => id,
        };
        let outcome = self
            .claim_inner(tenant, ticket, reservation, manifest.object)
            .await;
        if !matches!(outcome, Ok(RestoreClaim::Claimed(_))) {
            budget
                .release(tenant, reservation)
                .await
                .map_err(|_| RestoreFailure)?;
        }
        outcome.map_err(|_| RestoreFailure)
    }
    async fn complete(
        &self,
        lease: &RestoreLease,
        segment: &RestoredSegment,
    ) -> Result<bool, RestoreFailure> {
        self.publish(lease, segment)
            .await
            .map_err(|_| RestoreFailure)
    }
    async fn abandon(&self, lease: &RestoreLease) -> Result<(), RestoreFailure> {
        let mut tx = self.transaction().await.map_err(|_| RestoreFailure)?;
        lock(&mut tx, &lease.tenant)
            .await
            .map_err(|_| RestoreFailure)?;
        sqlx::query("UPDATE history.restore_jobs SET lease_until=clock_timestamp() WHERE tenant_id=$1 AND manifest_id=$2 AND request_generation=$3 AND attempt=$4 AND reservation_id=$5 AND status='pending'").bind(lease.tenant.as_str()).bind(lease.ticket.manifest()).bind(lease.ticket.generation()).bind(lease.attempt).bind(lease.reservation).execute(&mut *tx).await.map_err(|_|RestoreFailure)?;
        sqlx::query(
            "DELETE FROM history.work_reservations WHERE tenant_id=$1 AND reservation_id=$2",
        )
        .bind(lease.tenant.as_str())
        .bind(lease.reservation)
        .execute(&mut *tx)
        .await
        .map_err(|_| RestoreFailure)?;
        tx.commit().await.map_err(|_| RestoreFailure)
    }
}

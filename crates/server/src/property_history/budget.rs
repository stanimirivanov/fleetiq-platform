//! Bounded HTTP work; cancellation retains capacity until durable expiry.
use crate::protected_request::{self, ProtectedRequestError};
use axum::{
    body::{Body, to_bytes},
    http::StatusCode,
    response::Response,
};
use fleetiq_domain::TenantId;
use fleetiq_history::lifecycle::BudgetAdmission;
use fleetiq_postgres::PostgresHistoryBudgets;
use std::{future::Future, time::Duration};
pub(super) const RESPONSE_BYTES: usize = 1_048_576;
pub(super) fn denied() -> Response {
    protected_request::problem(
        StatusCode::TOO_MANY_REQUESTS,
        "urn:fleetiq:problem:history-budget-exceeded",
        "History work budget exceeded",
    )
}
pub(super) async fn run(
    budgets: &PostgresHistoryBudgets,
    tenant: &TenantId,
    work: impl Future<Output = Result<Response, ProtectedRequestError>>,
) -> Result<Response, ProtectedRequestError> {
    let admission = budgets
        .reserve(
            tenant,
            i64::try_from(RESPONSE_BYTES).map_err(|_| ProtectedRequestError::Internal)?,
        )
        .await
        .map_err(|_| ProtectedRequestError::Internal)?;
    let id = match admission {
        BudgetAdmission::Disabled => return work.await,
        BudgetAdmission::Denied => return Ok(denied()),
        BudgetAdmission::Reserved(id) => id,
    };
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let response = work.await?;
        let (parts, body) = response.into_parts();
        Ok(match to_bytes(body, RESPONSE_BYTES).await {
            Ok(bytes) => Response::from_parts(parts, Body::from(bytes)),
            Err(_) => denied(),
        })
    })
    .await
    .map_err(|_| ProtectedRequestError::Internal)?;
    // Timeout or dropped owner intentionally skips release: SQL may still be cancelling.
    budgets
        .release(tenant, id)
        .await
        .map_err(|_| ProtectedRequestError::Internal)?;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use sqlx::PgPool;
    #[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
    async fn errors_release_but_cancelled_owners_hold_capacity_until_expiry(pool: PgPool) {
        sqlx::raw_sql("INSERT INTO metadata.tenants VALUES ('tenant-a'); INSERT INTO identity.actors VALUES ('operator-a'); INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,1,1048576);").execute(&pool).await.unwrap();
        let tenant = TenantId::new("tenant-a").unwrap();
        let budgets = PostgresHistoryBudgets::new(pool.clone());
        assert!(
            run(&budgets, &tenant, async {
                Err(ProtectedRequestError::Internal)
            })
            .await
            .is_err()
        );
        let oversized = run(&budgets, &tenant, async {
            Ok(vec![0_u8; RESPONSE_BYTES + 1].into_response())
        })
        .await
        .unwrap();
        assert_eq!(oversized.status(), StatusCode::TOO_MANY_REQUESTS);
        let (started, ready) = tokio::sync::oneshot::channel();
        let owned_pool = pool.clone();
        let owned_tenant = tenant.clone();
        let task = tokio::spawn(async move {
            run(
                &PostgresHistoryBudgets::new(owned_pool),
                &owned_tenant,
                async {
                    started.send(()).unwrap();
                    std::future::pending::<Result<Response, ProtectedRequestError>>().await
                },
            )
            .await
        });
        ready.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(
            budgets.reserve(&tenant, 1).await.unwrap(),
            BudgetAdmission::Denied
        );
        sqlx::query("UPDATE history.work_reservations SET expires_at=clock_timestamp() WHERE tenant_id='tenant-a'").execute(&pool).await.unwrap();
        assert!(matches!(
            budgets.reserve(&tenant, 1).await.unwrap(),
            BudgetAdmission::Reserved(_)
        ));
    }
}

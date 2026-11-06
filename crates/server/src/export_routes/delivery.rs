//! Bounded attachment streaming; dropped or timed-out owners retain durable admission until expiry.
use super::{Context, Scope, authorize, dto, problem, ticket};
use crate::protected_request::{self, ProtectedRequestError, RequestAuthenticator};
use axum::{
    body::{Body, Bytes},
    extract::{
        Path, Query, State,
        rejection::{PathRejection, QueryRejection},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use fleetiq_domain::TenantId;
use fleetiq_history::{exports::ExportStatus, lifecycle::BudgetAdmission};
use fleetiq_postgres::PostgresHistoryBudgets;
use futures_util::stream::unfold;
use std::{io, sync::Arc, time::Duration};
use tokio::{sync::OwnedSemaphorePermit, time::Instant};
struct Transfer {
    pool: sqlx::PgPool,
    tenant: TenantId,
    reservation: i64,
    bytes: Bytes,
    permit: Option<OwnedSemaphorePermit>,
    deadline: Instant,
}
struct Slot {
    transfer: Option<Transfer>,
    expired: bool,
    release: Option<tokio::task::AbortHandle>,
    permit: Option<OwnedSemaphorePermit>,
}
struct Shared {
    slot: std::sync::Mutex<Slot>,
    watchdog: std::sync::Mutex<Option<tokio::task::AbortHandle>>,
}
impl Drop for Shared {
    fn drop(&mut self) {
        if let Some(watchdog) = self
            .watchdog
            .get_mut()
            .expect("private transfer mutex")
            .take()
        {
            watchdog.abort();
        }
    }
}
struct BodyOwner(Arc<Shared>);
impl Drop for BodyOwner {
    fn drop(&mut self) {
        if let Some(release) = self
            .0
            .slot
            .lock()
            .expect("private transfer mutex")
            .release
            .take()
        {
            release.abort();
        }
    }
}
fn watch(shared: &Arc<Shared>, deadline: Instant) {
    let weak = Arc::downgrade(shared);
    let watchdog = tokio::spawn(async move {
        tokio::time::sleep_until(deadline).await;
        if let Some(shared) = weak.upgrade() {
            let mut slot = shared.slot.lock().expect("private transfer mutex");
            slot.expired = true;
            slot.transfer.take();
            slot.permit.take();
            if let Some(release) = slot.release.take() {
                release.abort();
            }
        }
    });
    *shared.watchdog.lock().expect("private transfer mutex") = Some(watchdog.abort_handle());
}
fn stream(mut transfer: Transfer) -> Body {
    let permit = transfer.permit.take();
    let deadline = transfer.deadline;
    let shared = Arc::new(Shared {
        slot: std::sync::Mutex::new(Slot {
            transfer: Some(transfer),
            expired: false,
            release: None,
            permit,
        }),
        watchdog: std::sync::Mutex::new(None),
    });
    watch(&shared, deadline);
    Body::from_stream(unfold(Some(BodyOwner(shared)), |owner| async move {
        let owner = owner?;
        let shared = owner.0.clone();
        let (transfer, expired) = {
            let mut slot = shared.slot.lock().expect("private transfer mutex");
            (slot.transfer.take(), slot.expired)
        };
        if expired {
            return Some((
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "export transfer deadline",
                )),
                None,
            ));
        }
        let mut transfer = transfer?;
        if Instant::now() >= transfer.deadline {
            return Some((
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "export transfer deadline",
                )),
                None,
            ));
        }
        // Independent bounded frames cannot retain the entire source allocation in the transport.
        let count = transfer.bytes.len().min(65_536);
        let chunk = Bytes::copy_from_slice(&transfer.bytes[..count]);
        transfer.bytes = transfer.bytes.split_off(count);
        if transfer.bytes.is_empty() {
            transfer.bytes = Bytes::new();
            let (finished, completion) = tokio::sync::oneshot::channel();
            let retained = shared.clone();
            let deadline = transfer.deadline;
            // Independent polling bounds database cleanup even if the transport stops polling.
            let release = tokio::spawn(async move {
                let result = tokio::time::timeout_at(
                    deadline,
                    PostgresHistoryBudgets::new(transfer.pool)
                        .release(&transfer.tenant, transfer.reservation),
                )
                .await;
                drop(retained);
                let _ = finished.send(matches!(result, Ok(Ok(()))));
            });
            {
                let mut slot = shared.slot.lock().expect("private transfer mutex");
                if slot.expired {
                    release.abort();
                } else {
                    slot.release = Some(release.abort_handle());
                }
            }
            let released = completion.await.unwrap_or(false);
            if !released
                || Instant::now() >= deadline
                || shared.slot.lock().expect("private transfer mutex").expired
            {
                return Some((
                    Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "export transfer did not complete",
                    )),
                    None,
                ));
            }
            return Some((Ok(chunk), None));
        }
        {
            let mut slot = shared.slot.lock().expect("private transfer mutex");
            if slot.expired || Instant::now() >= transfer.deadline {
                return Some((
                    Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "export transfer deadline",
                    )),
                    None,
                ));
            }
            slot.transfer = Some(transfer);
        }
        Some((Ok(chunk), Some(owner)))
    }))
}
pub(super) async fn download<A: RequestAuthenticator>(
    State(ctx): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<Scope>, PathRejection>,
    query: Result<Query<dto::Empty>, QueryRejection>,
) -> Result<Response, ProtectedRequestError> {
    let deadline = Instant::now() + Duration::from_secs(5);
    tokio::time::timeout_at(deadline, prepare(ctx, headers, path, query, deadline))
        .await
        .map_err(|_| ProtectedRequestError::Internal)?
}
async fn prepare<A: RequestAuthenticator>(
    ctx: Arc<Context<A>>,
    headers: HeaderMap,
    path: Result<Path<Scope>, PathRejection>,
    query: Result<Query<dto::Empty>, QueryRejection>,
    deadline: Instant,
) -> Result<Response, ProtectedRequestError> {
    let read = authorize(&ctx, &headers, path.as_ref().map_or("", |p| p.0.0.as_str())).await?;
    let tenant = read.tenant_id().clone();
    let (Ok(Path((_, id))), Ok(Query(_))) = (path, query) else {
        return Ok(problem(StatusCode::BAD_REQUEST, "Invalid request"));
    };
    let Some(ticket) = ticket(&id) else {
        return Ok(problem(StatusCode::BAD_REQUEST, "Invalid export identity"));
    };
    let Some(info) = ctx
        .store
        .describe_authorized(read, ticket)
        .await
        .map_err(|_| ProtectedRequestError::Internal)?
    else {
        return Ok(problem(StatusCode::NOT_FOUND, "Resource not found"));
    };
    match info.status {
        ExportStatus::Ready => {}
        ExportStatus::Pending => return Ok(problem(StatusCode::CONFLICT, "Export not ready")),
        _ => return Ok(problem(StatusCode::GONE, "Export unavailable")),
    }
    if headers.contains_key(header::RANGE) {
        let mut r = problem(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "Export ranges are unsupported",
        );
        r.headers_mut().insert(
            header::CONTENT_RANGE,
            format!("bytes */{}", info.file_bytes)
                .parse()
                .map_err(|_| ProtectedRequestError::Internal)?,
        );
        return Ok(r);
    }
    let Ok(permit) = ctx.downloads.clone().try_acquire_owned() else {
        return Ok(problem(
            StatusCode::TOO_MANY_REQUESTS,
            "Export transfers busy",
        ));
    };
    let budget = PostgresHistoryBudgets::new(ctx.pool.clone());
    let BudgetAdmission::Reserved(reservation) = budget
        .reserve(&tenant, 2_097_152)
        .await
        .map_err(|_| ProtectedRequestError::Internal)?
    else {
        return Ok(problem(
            StatusCode::TOO_MANY_REQUESTS,
            "Export transfer budget unavailable",
        ));
    };
    let artifact = ctx.store.artifact(&tenant, ticket).await;
    let artifact = match artifact {
        Ok(Some(a)) => a,
        other => {
            budget
                .release(&tenant, reservation)
                .await
                .map_err(|_| ProtectedRequestError::Internal)?;
            return match other {
                Ok(None) => Ok(problem(StatusCode::GONE, "Export unavailable")),
                _ => Err(ProtectedRequestError::Internal),
            };
        }
    };
    let now = i64::try_from(time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000)
        .map_err(|_| ProtectedRequestError::Internal)?;
    let remaining = artifact.expires_at_ms().saturating_sub(now);
    let expires = Instant::now() + Duration::from_millis(u64::try_from(remaining).unwrap_or(0));
    let bytes = Bytes::from(artifact.into_bytes());
    let length = bytes.len();
    let body = stream(Transfer {
        pool: ctx.pool.clone(),
        tenant,
        reservation,
        bytes,
        permit: Some(permit),
        deadline: deadline.min(expires),
    });
    attachment(body, ticket.id(), length)
}
fn attachment(body: Body, id: i64, length: usize) -> Result<Response, ProtectedRequestError> {
    let mut response = body.into_response();
    let h = response.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        "application/vnd.apache.parquet"
            .parse()
            .expect("static header"),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"fleetiq-export-{id}.parquet\"")
            .parse()
            .map_err(|_| ProtectedRequestError::Internal)?,
    );
    h.insert(
        header::CONTENT_LENGTH,
        length
            .to_string()
            .parse()
            .map_err(|_| ProtectedRequestError::Internal)?,
    );
    h.insert(
        header::ACCEPT_RANGES,
        "none".parse().expect("static header"),
    );
    h.insert(
        "x-content-type-options",
        "nosniff".parse().expect("static header"),
    );
    Ok(protected_request::no_store(response))
}

pub(super) async fn head<A: RequestAuthenticator>(
    State(ctx): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<Scope>, PathRejection>,
    query: Result<Query<dto::Empty>, QueryRejection>,
) -> Result<Response, ProtectedRequestError> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let read = authorize(&ctx, &headers, path.as_ref().map_or("", |p| p.0.0.as_str())).await?;
        let (Ok(Path((_, id))), Ok(Query(_))) = (path, query) else {
            return Ok(problem(StatusCode::BAD_REQUEST, "Invalid request"));
        };
        let Some(ticket) = ticket(&id) else {
            return Ok(problem(StatusCode::BAD_REQUEST, "Invalid export identity"));
        };
        let Some(info) = ctx
            .store
            .describe_authorized(read, ticket)
            .await
            .map_err(|_| ProtectedRequestError::Internal)?
        else {
            return Ok(problem(StatusCode::NOT_FOUND, "Resource not found"));
        };
        match info.status {
            ExportStatus::Ready => attachment(
                Body::empty(),
                ticket.id(),
                usize::try_from(info.file_bytes).map_err(|_| ProtectedRequestError::Internal)?,
            ),
            ExportStatus::Pending => Ok(problem(StatusCode::CONFLICT, "Export not ready")),
            _ => Ok(problem(StatusCode::GONE, "Export unavailable")),
        }
    })
    .await
    .map_err(|_| ProtectedRequestError::Internal)?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn unpolled_transfer_expires_and_frees_buffer_capacity() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/test")
            .unwrap();
        let capacity = Arc::new(tokio::sync::Semaphore::new(1));
        let body = stream(Transfer {
            pool,
            tenant: TenantId::new("tenant-a").unwrap(),
            reservation: 1,
            bytes: Bytes::from(vec![0; 131_072]),
            permit: Some(capacity.clone().acquire_owned().await.unwrap()),
            deadline: Instant::now() + Duration::from_secs(5),
        });
        tokio::time::advance(Duration::from_secs(6)).await;
        tokio::task::yield_now().await;
        assert_eq!(capacity.available_permits(), 1);
        assert!(axum::body::to_bytes(body, 2_097_152).await.is_err());
    }
}

#[cfg(test)]
mod partial_tests {
    use super::*;
    use futures_util::StreamExt;
    #[tokio::test(start_paused = true)]
    async fn partial_body_cannot_resume_after_deadline() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/test")
            .unwrap();
        let capacity = Arc::new(tokio::sync::Semaphore::new(1));
        let body = stream(Transfer {
            pool,
            tenant: TenantId::new("tenant-a").unwrap(),
            reservation: 1,
            bytes: Bytes::from(vec![0; 131_072]),
            permit: Some(capacity.clone().acquire_owned().await.unwrap()),
            deadline: Instant::now() + Duration::from_secs(5),
        });
        let mut data = body.into_data_stream();
        assert_eq!(data.next().await.unwrap().unwrap().len(), 65_536);
        assert_eq!(capacity.available_permits(), 0);
        tokio::time::advance(Duration::from_secs(6)).await;
        tokio::task::yield_now().await;
        assert_eq!(capacity.available_permits(), 1);
        assert!(data.next().await.unwrap().is_err());
    }
}

#[cfg(test)]
mod final_frame_tests {
    use super::*;
    use futures_util::StreamExt;
    #[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
    async fn blocked_final_frame_cannot_pin_global_capacity_after_deadline(pool: sqlx::PgPool) {
        let limited = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_with((*pool.connect_options()).clone())
            .await
            .unwrap();
        let held = limited.acquire().await.unwrap();
        let capacity = Arc::new(tokio::sync::Semaphore::new(1));
        let body = stream(Transfer {
            pool: limited.clone(),
            tenant: TenantId::new("tenant-a").unwrap(),
            reservation: 1,
            bytes: Bytes::from(vec![0; 1024]),
            permit: Some(capacity.clone().acquire_owned().await.unwrap()),
            deadline: Instant::now() + Duration::from_secs(5),
        });
        tokio::time::pause();
        let mut data = body.into_data_stream();
        let mut next = Box::pin(data.next());
        assert!(futures_util::poll!(next.as_mut()).is_pending());
        tokio::time::advance(Duration::from_secs(6)).await;
        tokio::task::yield_now().await;
        assert_eq!(capacity.available_permits(), 1);
        assert!(next.await.unwrap().is_err());
        tokio::time::resume();
        drop(data);
        drop(held);
        limited.close().await;
    }
}

#[cfg(test)]
mod drop_tests {
    use super::*;
    use futures_util::StreamExt;
    #[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
    async fn dropped_final_frame_cancels_its_blocked_release_owner(pool: sqlx::PgPool) {
        sqlx::raw_sql("INSERT INTO metadata.tenants VALUES('tenant-a'); INSERT INTO identity.actors VALUES('operator-a'); INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES('tenant-a',1,'operator-a',7,30,2,4194304)").execute(&pool).await.unwrap();
        let tenant = TenantId::new("tenant-a").unwrap();
        let BudgetAdmission::Reserved(reservation) = PostgresHistoryBudgets::new(pool.clone())
            .reserve(&tenant, 2_097_152)
            .await
            .unwrap()
        else {
            panic!("reserve")
        };
        let mut blocker = pool.begin().await.unwrap();
        sqlx::query("SELECT reservation_id FROM history.work_reservations WHERE reservation_id=$1 FOR UPDATE").bind(reservation).fetch_one(&mut *blocker).await.unwrap();
        let capacity = Arc::new(tokio::sync::Semaphore::new(1));
        let body = stream(Transfer {
            pool: pool.clone(),
            tenant,
            reservation,
            bytes: Bytes::from(vec![0; 1024]),
            permit: Some(capacity.acquire_owned().await.unwrap()),
            deadline: Instant::now() + Duration::from_secs(5),
        });
        let mut data = body.into_data_stream();
        let mut next = Box::pin(data.next());
        assert!(futures_util::poll!(next.as_mut()).is_pending());
        tokio::time::timeout(Duration::from_secs(3),async{loop{let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'DELETE FROM history.work_reservations%')").fetch_one(&pool).await.unwrap();if waiting{break}tokio::task::yield_now().await;}}).await.unwrap();
        drop(next);
        drop(data);
        blocker.commit().await.unwrap();
        let mut barrier = pool.begin().await.unwrap();
        let retained=sqlx::query_scalar::<_,i64>("SELECT reservation_id FROM history.work_reservations WHERE reservation_id=$1 FOR UPDATE").bind(reservation).fetch_optional(&mut *barrier).await.unwrap();
        assert_eq!(retained, Some(reservation));
        barrier.rollback().await.unwrap();
    }
}

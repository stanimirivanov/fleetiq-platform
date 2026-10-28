//! PostgreSQL-backed HTTP authority and real TCP reconnect/revocation tests.
use super::*;
use axum::{body::Body, http::Request};
use fleetiq_access::CallerAuthenticationService;
use fleetiq_postgres::PostgresActorIdentityResolver;
use tower::ServiceExt;
const ROOT: &str =
    "/api/v1/tenants/tenant-a/assets/asset-a/properties/electrical.voltage/versions/1/live";
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
async fn setup(pool: &PgPool) -> Router {
    setup_with_capacity(pool, CONNECTION_LIMIT).await
}
async fn setup_with_capacity(pool: &PgPool, limit: usize) -> Router {
    sqlx::raw_sql(include_str!(
        "../../../postgres/tests/property_history/topology.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "operator")
        .unwrap();
    routes(Arc::new(Context {
        authentication: CallerAuthenticationService::new(
            crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
                config.bearer_identity().unwrap().clone(),
            ),
            PostgresActorIdentityResolver::new(pool.clone()),
        ),
        authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
            pool.clone(),
        )),
        current: CurrentStateService::new(PostgresCurrentStateRepository::new(pool.clone())),
        capacity: Arc::new(Semaphore::new(limit)),
    }))
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn handshake_checks_current_authority_before_cursor_or_scope(pool: PgPool) {
    let app = setup(&pool).await;
    for (path, authenticated, cursor, expected) in [
        (
            ROOT.to_owned(),
            false,
            Some("bad"),
            StatusCode::UNAUTHORIZED,
        ),
        (
            ROOT.replace("tenant-a", "tenant-b"),
            true,
            Some("bad"),
            StatusCode::FORBIDDEN,
        ),
        (
            ROOT.replace("/1/", "/invalid/"),
            true,
            None,
            StatusCode::BAD_REQUEST,
        ),
        (
            ROOT.replace("asset-a", "missing"),
            true,
            None,
            StatusCode::NOT_FOUND,
        ),
        (ROOT.to_owned(), true, Some("bad"), StatusCode::BAD_REQUEST),
    ] {
        let mut request = Request::get(path);
        if authenticated {
            request = request.header("authorization", format!("Bearer {TOKEN}"));
        }
        if let Some(cursor) = cursor {
            request = request.header("last-event-id", cursor);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    let response = app
        .oneshot(Request::post(ROOT).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}
mod network;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn capacity_is_bounded_and_disposal_releases_it(pool: PgPool) {
    let app = setup_with_capacity(&pool, 1).await;
    let request = || {
        Request::get(ROOT)
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap()
    };
    let held = app.clone().oneshot(request()).await.unwrap();
    assert_eq!(held.status(), StatusCode::OK);
    let busy = app.clone().oneshot(request()).await.unwrap();
    assert_eq!(busy.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(busy.headers()["retry-after"], "2");
    drop(held);
    let next = app.oneshot(request()).await.unwrap();
    assert_eq!(next.status(), StatusCode::OK);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn authorization_is_rechecked_before_the_first_body_payload(pool: PgPool) {
    use futures_util::StreamExt;
    let app = setup(&pool).await;
    let response = app
        .oneshot(
            Request::get(ROOT)
                .header("authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id = 'tenant-a' AND actor_id = 'operator-a' AND action = 'telemetry.read'").execute(&pool).await.unwrap();
    let mut body = response.into_body().into_data_stream();
    let bytes = body.next().await.unwrap().unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text.contains("access_denied"));
    assert!(!text.contains("event: snapshot"));
    assert!(!text.contains("id:"));
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn duplicate_and_cross_scope_resume_headers_are_rejected(pool: PgPool) {
    let app = setup(&pool).await;
    let tenant = TenantId::new("tenant-a").unwrap();
    let own_target =
        crate::current_state::target("asset-a".into(), "electrical.voltage".into(), "1").unwrap();
    let other_target =
        crate::current_state::target("asset-b".into(), "electrical.voltage".into(), "1").unwrap();
    let position = fleetiq_projection::DeliveryPosition {
        generation: 1,
        revision: 0,
    };
    let own_cursor = cursor::encode(&cursor::scope(&tenant, &own_target), position);
    let other_cursor = cursor::encode(&cursor::scope(&tenant, &other_target), position);
    for (value, duplicate) in [(other_cursor.as_str(), false), (own_cursor.as_str(), true)] {
        let mut request = Request::get(ROOT)
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("last-event-id", value);
        if duplicate {
            request = request.header("last-event-id", value);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn identity_reassignment_cannot_switch_the_actor_of_an_open_stream(pool: PgPool) {
    use futures_util::StreamExt;
    let app = setup(&pool).await;
    let response = app
        .oneshot(
            Request::get(ROOT)
                .header("authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    sqlx::raw_sql("INSERT INTO identity.actors (actor_id) VALUES ('replacement-operator'); INSERT INTO identity.tenant_action_grants (tenant_id, actor_id, action) VALUES ('tenant-a', 'replacement-operator', 'telemetry.read'); UPDATE identity.external_actor_identities SET actor_id = 'replacement-operator' WHERE authority_id = 'workload' AND subject = 'operator';")
        .execute(&pool).await.unwrap();
    let mut body = response.into_body().into_data_stream();
    let bytes = body.next().await.unwrap().unwrap();
    assert!(
        std::str::from_utf8(&bytes)
            .unwrap()
            .contains("authentication_required")
    );
}

//! Tenant commands use the selected browser security boundary, never bearer fallback.
use super::*;
use crate::mutation_guard::MutationGuard;
use serde_json::json;
fn mutation(path: &str, payload: &serde_json::Value) -> Request<Body> {
    Request::post(path)
        .header(
            header::COOKIE,
            secret_cookie(SESSION_COOKIE, SESSION_SECRET),
        )
        .header(header::ORIGIN, ORIGIN)
        .header("x-fleetiq-csrf", "1")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap()
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn tenant_commands_require_browser_guard_and_fresh_operation_authority(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    fleetiq_identity::PlatformBootstrapService::new(
        fleetiq_postgres::PostgresPlatformBootstrap::new(pool.clone()),
    )
    .bootstrap(&fleetiq_identity::PlatformBootstrapRequest::new(
        ActorId::new("actor-a")?,
        identity(),
        UnixTimeMillis::new(100),
    ))
    .await?;
    seed_session(&pool, UnixTimeMillis::new(now().unwrap().get() + 60_000)).await?;
    let app = crate::protected_application_router(
        pool.clone(),
        BrowserRequestAuthenticator::new(pool.clone(), &config()),
        MutationGuard::Browser {
            public_origin: ORIGIN.into(),
        },
    );
    let tenants = "/api/v1/admin/tenants";
    let lifecycle = "/api/v1/admin/tenants/tenant-a/lifecycle";
    let provision = json!({"provisioning_id":"browser-a","tenant_id":"tenant-a","initial_administrator_actor_id":"actor-a"});
    let suspend = json!({"change_id":"suspend-a","state":"suspended"});
    for (path, payload) in [(tenants, &provision), (lifecycle, &suspend)] {
        for name in ["origin", "x-fleetiq-csrf"] {
            let mut missing = mutation(path, payload);
            missing.headers_mut().remove(name);
            assert_eq!(
                app.clone().oneshot(missing).await?.status(),
                StatusCode::FORBIDDEN
            );
            let mut duplicate = mutation(path, payload);
            let value = duplicate.headers()[name].clone();
            duplicate.headers_mut().append(name, value);
            assert_eq!(
                app.clone().oneshot(duplicate).await?.status(),
                StatusCode::FORBIDDEN
            );
        }
        for (name, value) in [
            ("origin", "https://attacker.example"),
            ("x-fleetiq-csrf", "0"),
        ] {
            let mut bad = mutation(path, payload);
            bad.headers_mut().insert(name, value.parse()?);
            assert_eq!(
                app.clone().oneshot(bad).await?.status(),
                StatusCode::FORBIDDEN
            );
        }
        let mut mixed = mutation(path, payload);
        mixed.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {SESSION_SECRET}").parse()?,
        );
        assert_eq!(
            app.clone().oneshot(mixed).await?.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identity.tenant_provisionings")
            .fetch_one(&pool)
            .await?,
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identity.tenant_lifecycle_changes")
            .fetch_one(&pool)
            .await?,
        0
    );
    assert_eq!(
        app.clone()
            .oneshot(mutation(tenants, &provision))
            .await?
            .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        app.clone()
            .oneshot(mutation(lifecycle, &suspend))
            .await?
            .status(),
        StatusCode::OK
    );
    let cookie = secret_cookie(SESSION_COOKIE, SESSION_SECRET);
    assert_eq!(
        app.clone()
            .oneshot(request(tenants, Some(&cookie)))
            .await?
            .status(),
        StatusCode::OK
    );
    sqlx::query("DELETE FROM identity.platform_action_grants WHERE actor_id='actor-a' AND action='tenant.provision'").execute(&pool).await?;
    assert_eq!(
        app.clone()
            .oneshot(mutation(tenants, &provision))
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    let activate = json!({"change_id":"activate-a","state":"active"});
    assert_eq!(
        app.clone()
            .oneshot(mutation(lifecycle, &activate))
            .await?
            .status(),
        StatusCode::OK
    );
    sqlx::query("DELETE FROM identity.platform_action_grants WHERE actor_id='actor-a' AND action='tenant.lifecycle.manage'").execute(&pool).await?;
    for req in [
        mutation(lifecycle, &activate),
        request(tenants, Some(&cookie)),
        request("/api/v1/admin/tenant-lifecycle-changes", Some(&cookie)),
    ] {
        let response = app.clone().oneshot(req).await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert!(!response.headers().contains_key(header::WWW_AUTHENTICATE));
    }
    Ok(())
}

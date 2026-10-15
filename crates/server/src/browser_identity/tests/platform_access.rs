//! Browser platform grant commands reuse exact-origin/CSRF policy and fresh authority.
use super::*;
use crate::mutation_guard::MutationGuard;
use serde_json::json;
fn mutation(payload: &serde_json::Value) -> Request<Body> {
    Request::post("/api/v1/admin/actors/actor-a/platform-access-changes")
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
async fn platform_access_commands_require_browser_guard_and_recheck_authority(
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
    let payload = json!({"change_id":"grant-browser","kind":"grant","action":"tenant.provision"});
    for name in ["origin", "x-fleetiq-csrf"] {
        let mut missing = mutation(&payload);
        missing.headers_mut().remove(name);
        assert_eq!(
            app.clone().oneshot(missing).await?.status(),
            StatusCode::FORBIDDEN
        );
        let mut duplicate = mutation(&payload);
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
        let mut bad = mutation(&payload);
        bad.headers_mut().insert(name, value.parse()?);
        assert_eq!(
            app.clone().oneshot(bad).await?.status(),
            StatusCode::FORBIDDEN
        );
    }
    let mut mixed = mutation(&payload);
    mixed.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {SESSION_SECRET}").parse()?,
    );
    assert_eq!(
        app.clone().oneshot(mixed).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identity.platform_access_changes")
            .fetch_one(&pool)
            .await?,
        0
    );
    assert_eq!(
        app.clone().oneshot(mutation(&payload)).await?.status(),
        StatusCode::OK
    );
    for path in [
        "/api/v1/admin/actors/actor-a/platform-grants",
        "/api/v1/admin/platform-access-changes",
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request(
                    path,
                    Some(&secret_cookie(SESSION_COOKIE, SESSION_SECRET))
                ))
                .await?
                .status(),
            StatusCode::OK
        );
    }
    // Fixtures model a concurrent authority revocation; possession of the session is insufficient.
    sqlx::query("DELETE FROM identity.platform_action_grants WHERE actor_id='actor-a' AND action='platform.access.manage'").execute(&pool).await?;
    for mut req in [
        mutation(&payload),
        request(
            "/api/v1/admin/actors/actor-a/platform-grants",
            Some(&secret_cookie(SESSION_COOKIE, SESSION_SECRET)),
        ),
        request(
            "/api/v1/admin/platform-access-changes",
            Some(&secret_cookie(SESSION_COOKIE, SESSION_SECRET)),
        ),
    ] {
        req.headers_mut().remove(header::AUTHORIZATION);
        let response = app.clone().oneshot(req).await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(!response.headers().contains_key(header::WWW_AUTHENTICATE));
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    Ok(())
}

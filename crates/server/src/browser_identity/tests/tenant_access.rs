//! Tenant access commands use the listener-selected browser session and mutation guard.
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
async fn tenant_access_requires_browser_guard_and_current_tenant_authority(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_identity(&pool).await?;
    seed_session(&pool, UnixTimeMillis::new(now().unwrap().get() + 60_000)).await?;
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a')")
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-a','actor-a','access.manage')").execute(&pool).await?;
    let app = crate::protected_application_router(
        pool.clone(),
        BrowserRequestAuthenticator::new(pool.clone(), &config()),
        MutationGuard::Browser {
            public_origin: ORIGIN.into(),
        },
    );
    let path = "/api/v1/tenants/tenant-a/actors/actor-b/access-changes";
    let grants = "/api/v1/tenants/tenant-a/actors/actor-b/grants";
    let payload = json!({"change_id":"browser-grant","kind":"grant","action":"metadata.read"});
    for name in ["origin", "x-fleetiq-csrf"] {
        let mut missing = mutation(path, &payload);
        missing.headers_mut().remove(name);
        assert_eq!(
            app.clone().oneshot(missing).await?.status(),
            StatusCode::FORBIDDEN
        );
        let mut duplicate = mutation(path, &payload);
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
        let mut bad = mutation(path, &payload);
        bad.headers_mut().insert(name, value.parse()?);
        assert_eq!(
            app.clone().oneshot(bad).await?.status(),
            StatusCode::FORBIDDEN
        );
    }
    let mut mixed = mutation(path, &payload);
    mixed.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {SESSION_SECRET}").parse()?,
    );
    assert_eq!(
        app.clone().oneshot(mixed).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identity.tenant_access_changes")
            .fetch_one(&pool)
            .await?,
        0
    );
    assert_eq!(
        app.clone()
            .oneshot(mutation(path, &payload))
            .await?
            .status(),
        StatusCode::OK
    );
    let cookie = secret_cookie(SESSION_COOKIE, SESSION_SECRET);
    assert_eq!(
        app.clone()
            .oneshot(request(grants, Some(&cookie)))
            .await?
            .status(),
        StatusCode::OK
    );
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND actor_id='actor-a' AND action='access.manage'").execute(&pool).await?;
    for req in [
        mutation(path, &payload),
        request(grants, Some(&cookie)),
        request("/api/v1/tenants/tenant-a/access-changes", Some(&cookie)),
    ] {
        let response = app.clone().oneshot(req).await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert!(!response.headers().contains_key(header::WWW_AUTHENTICATE));
    }
    Ok(())
}

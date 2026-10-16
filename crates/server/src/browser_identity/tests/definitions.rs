//! Publication uses current browser sessions and the exact-origin mutation guard.
use super::*;
use crate::mutation_guard::MutationGuard;
use serde_json::json;
fn publication(path: &str, payload: &serde_json::Value) -> Request<Body> {
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
async fn definition_publication_requires_browser_guard_and_fresh_manage_metadata(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_identity(&pool).await?;
    seed_session(&pool, UnixTimeMillis::new(now().unwrap().get() + 60_000)).await?;
    sqlx::raw_sql("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a');
        INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-a','actor-a','metadata.read'),('tenant-a','actor-a','metadata.manage');").execute(&pool).await?;
    let app = crate::protected_application_router(
        pool.clone(),
        BrowserRequestAuthenticator::new(pool.clone(), &config()),
        MutationGuard::Browser {
            public_origin: ORIGIN.into(),
        },
    );
    let path = "/api/v1/tenants/tenant-a/properties";
    let payload = json!({"id":"enabled","version":1,"name":"Enabled","value_kind":"boolean"});
    for name in ["origin", "x-fleetiq-csrf"] {
        let mut missing = publication(path, &payload);
        missing.headers_mut().remove(name);
        assert_eq!(
            app.clone().oneshot(missing).await?.status(),
            StatusCode::FORBIDDEN
        );
        let mut duplicate = publication(path, &payload);
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
        let mut bad = publication(path, &payload);
        bad.headers_mut().insert(name, value.parse()?);
        assert_eq!(
            app.clone().oneshot(bad).await?.status(),
            StatusCode::FORBIDDEN
        );
    }
    let mut mixed = publication(path, &payload);
    mixed.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {SESSION_SECRET}").parse()?,
    );
    assert_eq!(
        app.clone().oneshot(mixed).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM metadata.property_definitions")
            .fetch_one(&pool)
            .await?,
        0
    );
    assert_eq!(
        app.clone()
            .oneshot(publication(path, &payload))
            .await?
            .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        app.clone()
            .oneshot(publication(path, &payload))
            .await?
            .status(),
        StatusCode::OK
    );
    let cookie = secret_cookie(SESSION_COOKIE, SESSION_SECRET);
    assert_eq!(
        app.clone()
            .oneshot(request(path, Some(&cookie)))
            .await?
            .status(),
        StatusCode::OK
    );
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE actor_id='actor-a' AND action='metadata.manage'").execute(&pool).await?;
    let denied = app.clone().oneshot(publication(path, &payload)).await?;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert_eq!(denied.headers()[header::CACHE_CONTROL], "no-store");
    assert!(!denied.headers().contains_key(header::WWW_AUTHENTICATE));
    assert_eq!(
        app.oneshot(request(path, Some(&cookie))).await?.status(),
        StatusCode::OK
    );
    Ok(())
}

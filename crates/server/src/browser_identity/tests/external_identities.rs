//! Mapping replacement must preserve browser mutation and current-login checks.
use super::*;
use crate::mutation_guard::MutationGuard;
use serde_json::json;

fn mutation(payload: &serde_json::Value) -> Request<Body> {
    Request::post("/api/v1/admin/actors/actor-a/external-identity-changes")
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
async fn identity_replacement_requires_browser_guard_and_detached_login_fails_closed(
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
    seed_session(
        &pool,
        UnixTimeMillis::new(now().expect("test clock").get() + 60_000),
    )
    .await?;
    let app = crate::protected_application_router(
        pool.clone(),
        BrowserRequestAuthenticator::new(pool.clone(), &config()),
        MutationGuard::Browser {
            public_origin: ORIGIN.into(),
        },
    );
    let attach = json!({"change_id":"attach-recovery","kind":"attach","external_identity":{"authority_id":"recovery","subject":" Recovery "}});
    for name in ["origin", "x-fleetiq-csrf"] {
        let mut missing = mutation(&attach);
        missing.headers_mut().remove(name);
        assert_eq!(
            app.clone().oneshot(missing).await?.status(),
            StatusCode::FORBIDDEN
        );
        let mut duplicate = mutation(&attach);
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
        let mut bad = mutation(&attach);
        bad.headers_mut().insert(name, value.parse()?);
        assert_eq!(
            app.clone().oneshot(bad).await?.status(),
            StatusCode::FORBIDDEN
        );
    }
    let mut mixed = mutation(&attach);
    mixed.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {SESSION_SECRET}").parse()?,
    );
    assert_eq!(
        app.clone().oneshot(mixed).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identity.external_identity_changes")
            .fetch_one(&pool)
            .await?,
        0
    );
    assert_eq!(
        app.clone().oneshot(mutation(&attach)).await?.status(),
        StatusCode::OK
    );
    let detach = json!({"change_id":"detach-login","kind":"detach","external_identity":{"authority_id":"workforce","subject":"operator-1"}});
    assert_eq!(
        app.clone().oneshot(mutation(&detach)).await?.status(),
        StatusCode::OK
    );
    // The session remains durable but no longer resolves through its original exact mapping.
    for (method, path) in [
        ("GET", "/api/v1/admin/actors/actor-a/external-identities"),
        ("GET", "/api/v1/admin/external-identity-changes"),
        (
            "POST",
            "/api/v1/admin/actors/actor-a/external-identity-changes",
        ),
    ] {
        let req = if method == "POST" {
            mutation(&detach)
        } else {
            request(path, Some(&secret_cookie(SESSION_COOKIE, SESSION_SECRET)))
        };
        let response = app.clone().oneshot(req).await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(!response.headers().contains_key(header::WWW_AUTHENTICATE));
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    Ok(())
}

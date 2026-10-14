//! Cookie-authenticated writes must pass fresh identity, exact origin, marker, and platform policy.
use super::*;
use crate::{
    actor_administration::postgres_actor_administration_router, mutation_guard::MutationGuard,
};
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
async fn administration_requires_exact_browser_mutation_headers_and_current_session(
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
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ('actor-b')")
        .execute(&pool)
        .await?;
    seed_session(
        &pool,
        UnixTimeMillis::new(now().expect("test clock").get() + 60_000),
    )
    .await?;
    sqlx::query("INSERT INTO identity.platform_action_grants (actor_id,action) VALUES ('actor-b','identity.manage'),('actor-b','platform.access.manage')").execute(&pool).await?;
    let app = postgres_actor_administration_router(
        pool.clone(),
        BrowserRequestAuthenticator::new(pool.clone(), &config()),
        MutationGuard::Browser {
            public_origin: ORIGIN.into(),
        },
    );
    let path = "/api/v1/admin/actors/actor-b/lifecycle";
    let payload = json!({"change_id":"change-a","state":"deactivated"});
    for missing in ["origin", "x-fleetiq-csrf"] {
        let mut request = mutation(path, &payload);
        request.headers_mut().remove(missing);
        let response = app.clone().oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    for duplicate in ["origin", "x-fleetiq-csrf"] {
        let mut request = mutation(path, &payload);
        let value = request.headers()[duplicate].clone();
        request.headers_mut().append(duplicate, value);
        assert_eq!(
            app.clone().oneshot(request).await?.status(),
            StatusCode::FORBIDDEN
        );
    }
    let mut cross_origin = mutation(path, &payload);
    cross_origin
        .headers_mut()
        .insert(header::ORIGIN, "https://attacker.example".parse()?);
    assert_eq!(
        app.clone().oneshot(cross_origin).await?.status(),
        StatusCode::FORBIDDEN
    );
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
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identity.actor_lifecycle_changes")
            .fetch_one(&pool)
            .await?,
        0
    );
    // Self-deactivation is legal with other recovery administrators. Its session
    // remains stored but must fail current identity resolution on the next call.
    let path = "/api/v1/admin/actors/actor-a/lifecycle";
    assert_eq!(
        app.clone()
            .oneshot(mutation(
                path,
                &json!({"change_id":"self-deactivate","state":"deactivated"})
            ))
            .await?
            .status(),
        StatusCode::OK
    );
    let response = app
        .oneshot(request(
            "/api/v1/admin/actors",
            Some(&secret_cookie(SESSION_COOKIE, SESSION_SECRET)),
        ))
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(!response.headers().contains_key(header::WWW_AUTHENTICATE));
    Ok(())
}

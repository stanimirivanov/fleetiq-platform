//! Browser registration shares the established session and mutation boundary.
use super::*;
use crate::mutation_guard::MutationGuard;
use serde_json::json;
fn command(path: &str, payload: &serde_json::Value) -> Request<Body> {
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
async fn both_registrations_require_exact_origin_csrf_and_current_manage_grant(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_identity(&pool).await?;
    seed_session(&pool, UnixTimeMillis::new(now().unwrap().get() + 60_000)).await?;
    sqlx::raw_sql("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a');
        INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-a','actor-a','metadata.manage');
        INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count) VALUES ('tenant-a','equipment',1,'Equipment',0);").execute(&pool).await?;
    let app = crate::protected_application_router(
        pool.clone(),
        BrowserRequestAuthenticator::new(pool.clone(), &config()),
        MutationGuard::Browser {
            public_origin: ORIGIN.into(),
        },
    );
    for (path, payload) in [
        (
            "/api/v1/tenants/tenant-a/assets",
            json!({"id":"asset-a","name":"Machine","asset_type":{"id":"equipment","version":1},"external_identifiers":[]}),
        ),
        (
            "/api/v1/tenants/tenant-a/assets/asset-a/device",
            json!({"endpoints":[]}),
        ),
    ] {
        for name in ["origin", "x-fleetiq-csrf"] {
            let mut missing = command(path, &payload);
            missing.headers_mut().remove(name);
            assert_eq!(
                app.clone().oneshot(missing).await?.status(),
                StatusCode::FORBIDDEN
            );
            let mut duplicate = command(path, &payload);
            let value = duplicate.headers()[name].clone();
            duplicate.headers_mut().append(name, value);
            assert_eq!(
                app.clone().oneshot(duplicate).await?.status(),
                StatusCode::FORBIDDEN
            );
        }
        let mut wrong = command(path, &payload);
        wrong
            .headers_mut()
            .insert(header::ORIGIN, "https://attacker.example".parse()?);
        assert_eq!(
            app.clone().oneshot(wrong).await?.status(),
            StatusCode::FORBIDDEN
        );
        let mut mixed = command(path, &payload);
        mixed.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {SESSION_SECRET}").parse()?,
        );
        assert_eq!(
            app.clone().oneshot(mixed).await?.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            app.clone().oneshot(command(path, &payload)).await?.status(),
            StatusCode::CREATED
        );
        assert_eq!(
            app.clone().oneshot(command(path, &payload)).await?.status(),
            StatusCode::OK
        );
    }
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='metadata.manage'")
        .execute(&pool)
        .await?;
    let denied = app
        .oneshot(command(
            "/api/v1/tenants/tenant-a/assets/asset-a/device",
            &json!({"endpoints":[]}),
        ))
        .await?;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert!(!denied.headers().contains_key(header::WWW_AUTHENTICATE));
    assert_eq!(denied.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM metadata.registration_records")
            .fetch_one(&pool)
            .await?,
        2
    );
    Ok(())
}

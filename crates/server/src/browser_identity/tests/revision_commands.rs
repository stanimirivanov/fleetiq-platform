//! Both revision commands use the existing same-origin session mutation boundary.
use super::*;
use crate::mutation_guard::MutationGuard;
use serde_json::json;
fn command(binding: bool) -> Request<Body> {
    let (resource, payload) = if binding {
        (
            "signal-bindings",
            json!({"source":{"device_asset_id":"asset-a","endpoint_id":"can","signal_id":"bus.voltage"},"target":{"asset_id":"asset-b","property":{"id":"voltage","version":1}},"effective_interval":{"start_ms":0}}),
        )
    } else {
        (
            "relationships",
            json!({"relationship_type":{"id":"physical.contains","version":1},"source_asset_id":"asset-a","target_asset_id":"asset-b","effective_interval":{"start_ms":0}}),
        )
    };
    Request::post(format!(
        "/api/v1/tenants/tenant-a/{resource}/fact-a/revisions/1"
    ))
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
async fn revision_commands_require_exact_origin_csrf_and_fresh_session_authority(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_identity(&pool).await?;
    seed_session(&pool, UnixTimeMillis::new(now().unwrap().get() + 60_000)).await?;
    sqlx::raw_sql(include_str!("../../revision_commands/tests/topology.sql"))
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-a','actor-a','metadata.manage')").execute(&pool).await?;
    let app = crate::protected_application_router(
        pool.clone(),
        BrowserRequestAuthenticator::new(pool.clone(), &config()),
        MutationGuard::Browser {
            public_origin: ORIGIN.into(),
        },
    );
    for binding in [false, true] {
        for name in ["origin", "x-fleetiq-csrf"] {
            let mut req = command(binding);
            req.headers_mut().remove(name);
            assert_eq!(
                app.clone().oneshot(req).await?.status(),
                StatusCode::FORBIDDEN
            );
            let mut req = command(binding);
            let value = req.headers()[name].clone();
            req.headers_mut().append(name, value);
            assert_eq!(
                app.clone().oneshot(req).await?.status(),
                StatusCode::FORBIDDEN
            );
        }
        let mut req = command(binding);
        req.headers_mut()
            .insert(header::ORIGIN, "https://attacker.example".parse()?);
        assert_eq!(
            app.clone().oneshot(req).await?.status(),
            StatusCode::FORBIDDEN
        );
        let mut req = command(binding);
        req.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {SESSION_SECRET}").parse()?,
        );
        assert_eq!(
            app.clone().oneshot(req).await?.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            app.clone().oneshot(command(binding)).await?.status(),
            StatusCode::CREATED
        );
        assert_eq!(
            app.clone().oneshot(command(binding)).await?.status(),
            StatusCode::OK
        );
    }
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE actor_id='actor-a' AND action='metadata.manage'").execute(&pool).await?;
    for binding in [false, true] {
        let response = app.clone().oneshot(command(binding)).await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    let actors:Vec<String>=sqlx::query_scalar("SELECT actor_id FROM metadata.relationship_revision_records UNION ALL SELECT actor_id FROM metadata.signal_binding_revision_records").fetch_all(&pool).await?;
    assert_eq!(actors, vec!["actor-a", "actor-a"]);
    Ok(())
}

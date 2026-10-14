//! Browser discovery uses the same fresh identity and policy boundaries as reads.
use super::*;
use serde_json::{Value, json};

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn browser_discovery_rechecks_access_and_cannot_authorize_a_later_read(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_identity(&pool).await?;
    seed_session(
        &pool,
        UnixTimeMillis::new(now().expect("test clock").get() + 60_000),
    )
    .await?;
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a'),('tenant-b')")
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-a','actor-a','metadata.read'),('tenant-b','actor-b','metadata.read')").execute(&pool).await?;
    let authentication = BrowserRequestAuthenticator::new(pool.clone(), &config());
    let app = crate::operator_tenants::postgres_operator_tenants_router(
        pool.clone(),
        authentication.clone(),
    )
    .merge(crate::protected_asset::postgres_asset_router(
        pool.clone(),
        authentication,
    ));
    let cookie = secret_cookie(SESSION_COOKIE, SESSION_SECRET);
    let path = "/api/v1/me/tenants";
    let response = app.clone().oneshot(request(path, Some(&cookie))).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let body = to_bytes(response.into_body(), 8192).await?;
    crate::contract_test_support::assert_published_example(&body, path);
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE actor_id='actor-a'")
        .execute(&pool)
        .await?;
    let response = app.clone().oneshot(request(path, Some(&cookie))).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<Value>(&to_bytes(response.into_body(), 8192).await?)?,
        json!({"tenants":[],"next_after":null})
    );
    let denied = app
        .clone()
        .oneshot(request("/api/v1/tenants/tenant-a/assets", Some(&cookie)))
        .await?;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    sqlx::query("UPDATE identity.actors SET is_active=false WHERE actor_id='actor-a'")
        .execute(&pool)
        .await?;
    let inactive = app.clone().oneshot(request(path, Some(&cookie))).await?;
    assert_eq!(inactive.status(), StatusCode::UNAUTHORIZED);
    assert!(!inactive.headers().contains_key(header::WWW_AUTHENTICATE));
    let anonymous = app.oneshot(request(path, None)).await?;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn browser_discovery_paginates_and_filters_suspended_tenants(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_identity(&pool).await?;
    seed_session(
        &pool,
        UnixTimeMillis::new(now().expect("test clock").get() + 60_000),
    )
    .await?;
    sqlx::query(
        "INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a'),('tenant-b'),('tenant-c')",
    )
    .execute(&pool)
    .await?;
    sqlx::query("INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-a','actor-a','metadata.read'),('tenant-a','actor-a','access.manage'),('tenant-b','actor-a','metadata.manage'),('tenant-c','actor-a','metadata.read')").execute(&pool).await?;
    let app = crate::operator_tenants::postgres_operator_tenants_router(
        pool.clone(),
        BrowserRequestAuthenticator::new(pool.clone(), &config()),
    );
    let cookie = secret_cookie(SESSION_COOKIE, SESSION_SECRET);
    let response = app
        .clone()
        .oneshot(request("/api/v1/me/tenants?limit=1", Some(&cookie)))
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let first: Value = serde_json::from_slice(&to_bytes(response.into_body(), 8192).await?)?;
    assert_eq!(
        first,
        json!({"tenants":[{"tenant_id":"tenant-a","actions":["access.manage","metadata.read"]}],"next_after":"tenant-a"})
    );
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-b'")
        .execute(&pool)
        .await?;
    let response = app
        .clone()
        .oneshot(request(
            "/api/v1/me/tenants?limit=1&after=tenant-a",
            Some(&cookie),
        ))
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let second: Value = serde_json::from_slice(&to_bytes(response.into_body(), 8192).await?)?;
    assert_eq!(
        second,
        json!({"tenants":[{"tenant_id":"tenant-c","actions":["metadata.read"]}],"next_after":null})
    );
    // Reassigning the external identity must not transfer the existing session.
    sqlx::query("UPDATE identity.external_actor_identities SET actor_id='actor-b' WHERE authority_id='workforce' AND subject='operator-1'").execute(&pool).await?;
    let response = app
        .oneshot(request("/api/v1/me/tenants", Some(&cookie)))
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

//! Current tenant permissions under browser session authentication.

use super::*;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn browser_metadata_requests_enforce_current_grants_and_tenant_isolation(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_identity(&pool).await?;
    seed_session(
        &pool,
        UnixTimeMillis::new(now().expect("test clock").get() + 60_000),
    )
    .await?;
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a'), ('tenant-b')")
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO identity.tenant_action_grants (tenant_id, actor_id, action) VALUES ('tenant-a', 'actor-a', 'metadata.read')").execute(&pool).await?;
    let app = crate::protected_asset::postgres_asset_router(
        pool.clone(),
        BrowserRequestAuthenticator::new(pool.clone(), &config()),
    );
    let cookie = secret_cookie(SESSION_COOKIE, SESSION_SECRET);
    let permitted = app
        .clone()
        .oneshot(request("/api/v1/tenants/tenant-a/assets", Some(&cookie)))
        .await?;
    assert_eq!(permitted.status(), StatusCode::OK);
    let other_tenant = app
        .clone()
        .oneshot(request("/api/v1/tenants/tenant-b/assets", Some(&cookie)))
        .await?;
    assert_eq!(other_tenant.status(), StatusCode::FORBIDDEN);
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id = 'tenant-a' AND actor_id = 'actor-a' AND action = 'metadata.read'").execute(&pool).await?;
    let revoked = app
        .clone()
        .oneshot(request("/api/v1/tenants/tenant-a/assets", Some(&cookie)))
        .await?;
    assert_eq!(revoked.status(), StatusCode::FORBIDDEN);
    let unauthenticated = app
        .oneshot(request("/api/v1/tenants/tenant-a/assets", None))
        .await?;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    assert!(
        !unauthenticated
            .headers()
            .contains_key(header::WWW_AUTHENTICATE)
    );
    Ok(())
}

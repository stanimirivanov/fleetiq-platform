use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn provisioning_and_lifecycle_permissions_are_distinct_and_current(pool: PgPool) {
    let app = setup(&pool).await;
    sqlx::query("DELETE FROM identity.platform_action_grants WHERE actor_id='operator-a' AND action='tenant.provision'").execute(&pool).await.unwrap();
    assert_eq!(
        app.clone()
            .oneshot(request("POST", TENANTS, &json!({})))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.clone()
            .oneshot(request("GET", TENANTS, &Value::Null))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    sqlx::query("INSERT INTO identity.platform_action_grants (actor_id,action) VALUES ('operator-a','tenant.provision')").execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM identity.platform_action_grants WHERE actor_id='operator-a' AND action='tenant.lifecycle.manage'").execute(&pool).await.unwrap();
    create(&app, "tenant-a").await;
    for (method, path) in [("GET", TENANTS), ("GET", AUDIT), ("POST", LIFECYCLE)] {
        assert_eq!(
            app.clone()
                .oneshot(request(method, path, &json!({})))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    // All three tenant permissions are insufficient for any platform administration.
    let tenant_admin = router(&pool, "administrator");
    for (method, path) in [
        ("GET", TENANTS),
        ("GET", AUDIT),
        ("POST", TENANTS),
        ("POST", LIFECYCLE),
    ] {
        assert_eq!(
            tenant_admin
                .clone()
                .oneshot(request(method, path, &json!({})))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let mut anonymous = request(method, path, &json!({}));
        anonymous.headers_mut().remove(header::AUTHORIZATION);
        let response = app.clone().oneshot(anonymous).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().contains_key(header::WWW_AUTHENTICATE));
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn unsupported_methods_and_unconfigured_listener_have_safe_boundaries(pool: PgPool) {
    let app = setup(&pool).await;
    for (method, path, allow) in [
        ("DELETE", TENANTS, "GET, HEAD, POST"),
        ("GET", LIFECYCLE, "POST"),
        ("POST", AUDIT, "GET, HEAD"),
    ] {
        let response = app
            .clone()
            .oneshot(request(method, path, &Value::Null))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.headers()[header::ALLOW], allow);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    let cfg = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0").unwrap();
    let app = crate::application_router(pool, &cfg).await.unwrap();
    for path in [TENANTS, AUDIT, LIFECYCLE] {
        assert_eq!(
            app.clone()
                .oneshot(request("GET", path, &Value::Null))
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
    }
}

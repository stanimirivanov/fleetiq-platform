use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn catalogue_and_audit_examples_match_and_cursors_handle_ties(pool: PgPool) {
    let app = setup(&pool).await;
    create(&app, "tenant-a").await;
    let response = app
        .clone()
        .oneshot(request("GET", TENANTS, &Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    crate::contract_test_support::assert_published_example(
        &to_bytes(response.into_body(), 32768).await.unwrap(),
        TENANTS,
    );
    let authorization = app_state_authorization(&pool).await;
    TenantLifecycleAdministration::new(PostgresTenantLifecycleAdministration::new(pool.clone()))
        .apply(
            authorization,
            fleetiq_identity::TenantLifecycleChangeRequest::new(
                fleetiq_domain::TenantLifecycleChangeId::new("change-a").unwrap(),
                fleetiq_domain::TenantId::new("tenant-a").unwrap(),
                fleetiq_identity::TenantLifecycleChangeKind::Suspend,
                UnixTimeMillis::new(1000),
            ),
        )
        .await
        .unwrap();
    let response = app
        .clone()
        .oneshot(request("GET", AUDIT, &Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    crate::contract_test_support::assert_published_example(
        &to_bytes(response.into_body(), 32768).await.unwrap(),
        AUDIT,
    );
    TenantLifecycleAdministration::new(PostgresTenantLifecycleAdministration::new(pool.clone()))
        .apply(
            app_state_authorization(&pool).await,
            fleetiq_identity::TenantLifecycleChangeRequest::new(
                fleetiq_domain::TenantLifecycleChangeId::new("change-b").unwrap(),
                fleetiq_domain::TenantId::new("tenant-a").unwrap(),
                fleetiq_identity::TenantLifecycleChangeKind::Activate,
                UnixTimeMillis::new(1000),
            ),
        )
        .await
        .unwrap();
    create(&app, "tenant-b").await;
    let first = body(
        app.clone()
            .oneshot(request("GET", &format!("{TENANTS}?limit=1"), &Value::Null))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        first,
        json!({"tenants":[{"tenant_id":"tenant-a","state":"active"}],"next_after":"tenant-a"})
    );
    let next = body(
        app.clone()
            .oneshot(request(
                "GET",
                &format!("{TENANTS}?limit=1&after=tenant-a"),
                &Value::Null,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        next,
        json!({"tenants":[{"tenant_id":"tenant-b","state":"active"}],"next_after":null})
    );
    let first = body(
        app.clone()
            .oneshot(request("GET", &format!("{AUDIT}?limit=1"), &Value::Null))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(first["changes"][0]["change_id"], "change-b");
    assert_eq!(
        first["next_before"],
        json!({"recorded_at_ms":1000,"change_id":"change-b"})
    );
    let next = body(
        app.clone()
            .oneshot(request(
                "GET",
                &format!("{AUDIT}?limit=1&before_recorded_at_ms=1000&before_change_id=change-b"),
                &Value::Null,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(next["changes"][0]["change_id"], "change-a");
    assert_eq!(next["next_before"], Value::Null);
}
async fn app_state_authorization(
    pool: &PgPool,
) -> fleetiq_access::AuthorizedPlatform<fleetiq_access::ManageTenantLifecycle> {
    let cfg = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "operator")
        .unwrap();
    let auth = CallerAuthenticationService::new(
        crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
            cfg.bearer_identity().unwrap().clone(),
        ),
        PostgresActorIdentityResolver::new(pool.clone()),
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {TOKEN}").parse().unwrap(),
    );
    PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
        .authorize::<fleetiq_access::ManageTenantLifecycle>(
            protected_request::authenticate(&headers, &auth)
                .await
                .unwrap(),
        )
        .await
        .unwrap()
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn malformed_pages_and_bodies_are_rejected_without_writes(pool: PgPool) {
    let app = setup(&pool).await;
    for path in [
        format!("{TENANTS}?limit=0"),
        format!("{TENANTS}?limit=101"),
        format!("{TENANTS}?after="),
        format!("{TENANTS}?unknown=1"),
        format!("{AUDIT}?before_change_id=a"),
        format!("{AUDIT}?before_recorded_at_ms=1"),
        format!("{AUDIT}?limit=101"),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request("GET", &path, &Value::Null))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    for (path, payload) in [
        (
            TENANTS,
            json!({"provisioning_id":"","tenant_id":"a","initial_administrator_actor_id":"admin-a"}),
        ),
        (
            TENANTS,
            json!({"provisioning_id":"a","tenant_id":"a","initial_administrator_actor_id":"admin-a","recorded_at_ms":1}),
        ),
        (LIFECYCLE, lifecycle("a", "deactivated")),
        (
            LIFECYCLE,
            json!({"change_id":"a","state":"active","administrator_actor_id":"admin-a"}),
        ),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request("POST", path, &payload))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let mut oversized = request("POST", TENANTS, &Value::Null);
    *oversized.body_mut() = Body::from("x".repeat(16385));
    assert_eq!(
        app.clone().oneshot(oversized).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identity.tenant_provisionings")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}

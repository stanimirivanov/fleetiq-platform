//! Bounded current state and immutable audit are deliberately independent.
use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn grant_pages_follow_exact_action_order_and_deleted_cursors_continue(pool: PgPool) {
    let app = setup(&pool).await;
    for (id, action) in [
        ("one", "access.manage"),
        ("two", "metadata.manage"),
        ("three", "metadata.read"),
    ] {
        send(&app, "POST", COMMANDS, &change(id, "grant", action)).await;
    }
    let response = app
        .clone()
        .oneshot(request("GET", GRANTS, &Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
    crate::contract_test_support::assert_published_example(
        &bytes,
        "/api/v1/tenants/{tenant_id}/actors/{actor_id}/grants",
    );
    let first = send(&app, "GET", &format!("{GRANTS}?limit=1"), &Value::Null).await;
    assert_eq!(
        first,
        json!({"grants":[{"action":"access.manage"}],"next_after":"access.manage"})
    );
    send(
        &app,
        "POST",
        COMMANDS,
        &change("remove", "revoke", "access.manage"),
    )
    .await;
    let next = send(
        &app,
        "GET",
        &format!("{GRANTS}?limit=1&after_action=access.manage"),
        &Value::Null,
    )
    .await;
    assert_eq!(next["grants"], json!([{"action":"metadata.manage"}]));
    let last = send(
        &app,
        "GET",
        &format!("{GRANTS}?after_action=metadata.manage"),
        &Value::Null,
    )
    .await;
    assert_eq!(
        last,
        json!({"grants":[{"action":"metadata.read"}],"next_after":null})
    );
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/tenants/tenant-a/actors/missing/grants",
            &Value::Null
        )
        .await,
        json!({"grants":[],"next_after":null})
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn audit_pages_use_first_receipt_time_and_tie_break_by_identity(pool: PgPool) {
    let app = setup(&pool).await;
    for id in ["change-a", "change-b"] {
        // Same host timestamp proves keyset tie behavior without sleeps or clock assumptions.
        let authorization =
            TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone()))
                .authorize::<ManageAccess>(
                    app_actor(&pool).await,
                    fleetiq_domain::TenantId::new("tenant-a").unwrap(),
                )
                .await
                .unwrap();
        TenantAccessAdministration::new(PostgresTenantAccessAdministration::new(pool.clone()))
            .apply(
                authorization,
                fleetiq_identity::TenantAccessChangeRequest::new(
                    fleetiq_domain::TenantAccessChangeId::new(id).unwrap(),
                    ActorId::new("actor-b").unwrap(),
                    fleetiq_access::TenantAction::ReadMetadata,
                    fleetiq_identity::TenantAccessChangeKind::Grant,
                    UnixTimeMillis::new(1000),
                ),
            )
            .await
            .unwrap();
    }
    let first = send(&app, "GET", &format!("{AUDIT}?limit=1"), &Value::Null).await;
    assert_eq!(
        first["changes"][0],
        json!({"change_id":"change-b","administrator_actor_id":"operator-a",
        "target_actor_id":"actor-b","tenant_id":"tenant-a","action":"metadata.read","kind":"grant","recorded_at_ms":1000,"state_changed":false})
    );
    assert_eq!(
        first["next_before"],
        json!({"recorded_at_ms":1000,"change_id":"change-b"})
    );
    crate::contract_test_support::assert_published_example(
        &serde_json::to_vec(&first).unwrap(),
        "/api/v1/tenants/{tenant_id}/access-changes",
    );
    let older = send(
        &app,
        "GET",
        &format!("{AUDIT}?limit=1&before_recorded_at_ms=1000&before_change_id=change-b"),
        &Value::Null,
    )
    .await;
    assert_eq!(older["changes"][0]["change_id"], "change-a");
    assert_eq!(older["changes"][0]["state_changed"], true);
    assert_eq!(older["next_before"], Value::Null);
}
async fn app_actor(pool: &PgPool) -> fleetiq_access::AuthenticatedActor {
    let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "operator")
        .unwrap();
    CallerAuthenticationService::new(
        crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
            config.bearer_identity().unwrap().clone(),
        ),
        PostgresActorIdentityResolver::new(pool.clone()),
    )
    .authenticate(&TOKEN.to_owned())
    .await
    .unwrap()
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn invalid_queries_methods_and_unconfigured_routes_are_bounded(pool: PgPool) {
    let app = setup(&pool).await;
    for path in [
        format!("{GRANTS}?limit=0"),
        format!("{GRANTS}?limit=101"),
        format!("{GRANTS}?limit=1&limit=2"),
        format!("{GRANTS}?after_action=identity.manage"),
        format!("{GRANTS}?after_action="),
        format!("{GRANTS}?after_action=access.manage&after_action=identity.manage"),
        format!("{GRANTS}?unknown=1"),
        format!("{AUDIT}?before_change_id=id"),
        format!("{AUDIT}?before_recorded_at_ms=1"),
        format!("{AUDIT}?before_recorded_at_ms=1&before_change_id="),
        format!("{AUDIT}?limit=101"),
        format!("{AUDIT}?actor_id=other"),
    ] {
        let response = app
            .clone()
            .oneshot(request("GET", &path, &Value::Null))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        body(response).await;
    }
    for (method, path, allow) in [
        ("POST", GRANTS, "GET, HEAD"),
        ("GET", COMMANDS, "POST"),
        ("POST", AUDIT, "GET, HEAD"),
    ] {
        let response = app
            .clone()
            .oneshot(request(method, path, &Value::Null))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.headers()[header::ALLOW], allow);
        body(response).await;
    }
    let head = app
        .oneshot(request("HEAD", GRANTS, &Value::Null))
        .await
        .unwrap();
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.headers()[header::CACHE_CONTROL], "no-store");
    assert!(to_bytes(head.into_body(), 8192).await.unwrap().is_empty());
    let config =
        crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0").unwrap();
    let unconfigured = crate::application_router(pool, &config).await.unwrap();
    for path in [GRANTS, COMMANDS, AUDIT] {
        assert_eq!(
            unconfigured
                .clone()
                .oneshot(request("GET", path, &Value::Null))
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
    }
}

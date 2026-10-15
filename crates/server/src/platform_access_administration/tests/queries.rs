//! Bounded current state and immutable audit are deliberately independent.
use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn grant_pages_follow_exact_action_order_and_deleted_cursors_continue(pool: PgPool) {
    let app = setup(&pool).await;
    for (id, action) in [
        ("one", "identity.manage"),
        ("two", "platform.access.manage"),
        ("three", "tenant.lifecycle.manage"),
        ("four", "tenant.provision"),
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
        "/api/v1/admin/actors/{actor_id}/platform-grants",
    );
    let first = send(&app, "GET", &format!("{GRANTS}?limit=1"), &Value::Null).await;
    assert_eq!(
        first,
        json!({"grants":[{"action":"identity.manage"}],"next_after":"identity.manage"})
    );
    send(
        &app,
        "POST",
        COMMANDS,
        &change("remove", "revoke", "identity.manage"),
    )
    .await;
    let next = send(
        &app,
        "GET",
        &format!("{GRANTS}?limit=1&after_action=identity.manage"),
        &Value::Null,
    )
    .await;
    assert_eq!(next["grants"], json!([{"action":"platform.access.manage"}]));
    let last = send(
        &app,
        "GET",
        &format!("{GRANTS}?after_action=tenant.lifecycle.manage"),
        &Value::Null,
    )
    .await;
    assert_eq!(
        last,
        json!({"grants":[{"action":"tenant.provision"}],"next_after":null})
    );
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/admin/actors/missing/platform-grants",
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
            PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
                .authorize::<ManagePlatformAccess>(app_actor(&pool).await)
                .await
                .unwrap();
        PlatformAccessAdministration::new(PostgresPlatformAccessAdministration::new(pool.clone()))
            .apply(
                authorization,
                fleetiq_identity::PlatformAccessChangeRequest::new(
                    fleetiq_domain::PlatformAccessChangeId::new(id).unwrap(),
                    ActorId::new("actor-b").unwrap(),
                    fleetiq_access::PlatformAction::ProvisionTenant,
                    fleetiq_identity::PlatformAccessChangeKind::Grant,
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
        "target_actor_id":"actor-b","action":"tenant.provision","kind":"grant","recorded_at_ms":1000,"state_changed":false})
    );
    assert_eq!(
        first["next_before"],
        json!({"recorded_at_ms":1000,"change_id":"change-b"})
    );
    crate::contract_test_support::assert_published_example(
        &serde_json::to_vec(&first).unwrap(),
        "/api/v1/admin/platform-access-changes",
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
        format!("{GRANTS}?after_action=metadata.read"),
        format!("{GRANTS}?after_action="),
        format!("{GRANTS}?after_action=identity.manage&after_action=tenant.provision"),
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
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn current_exact_authority_precedes_validation_for_every_operation(pool: PgPool) {
    let app = setup(&pool).await;
    // identity.manage and tenant.provision remain present but cannot authorize these routes.
    sqlx::query("DELETE FROM identity.platform_action_grants WHERE actor_id='operator-a' AND action='platform.access.manage'").execute(&pool).await.unwrap();
    for (method, path, payload) in [
        ("GET", format!("{GRANTS}?limit=0"), Value::Null),
        ("GET", AUDIT.to_owned(), Value::Null),
        ("POST", COMMANDS.to_owned(), json!({})),
    ] {
        let response = app
            .clone()
            .oneshot(request(method, &path, &payload))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        body(response).await;
        let mut anonymous = request(method, &path, &payload);
        anonymous.headers_mut().remove(header::AUTHORIZATION);
        let response = app.clone().oneshot(anonymous).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().contains_key(header::WWW_AUTHENTICATE));
        body(response).await;
    }
    assert_eq!(audit_count(&pool).await, 0);
}

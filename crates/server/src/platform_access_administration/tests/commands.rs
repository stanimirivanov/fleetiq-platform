//! Grant mutation, collision, rollback, and lockout behavior over the composed HTTP boundary.
use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_http_retries_keep_one_fact_and_never_restore_a_revoked_grant(pool: PgPool) {
    let app = setup(&pool).await;
    let payload = change("grant-a", "grant", "tenant.provision");
    let (a, b) = tokio::join!(
        app.clone().oneshot(request("POST", COMMANDS, &payload)),
        app.clone().oneshot(request("POST", COMMANDS, &payload)),
    );
    let mut outcomes = Vec::new();
    for response in [a.unwrap(), b.unwrap()] {
        assert_eq!(response.status(), StatusCode::OK);
        outcomes.push(body(response).await);
    }
    assert!(outcomes.contains(&json!({"outcome":"recorded","state_changed":true})));
    assert!(outcomes.contains(&json!({"outcome":"already_recorded","state_changed":true})));
    assert_eq!(audit_count(&pool).await, 1);
    assert_eq!(
        send(
            &app,
            "POST",
            COMMANDS,
            &change("noop", "grant", "tenant.provision")
        )
        .await,
        json!({"outcome":"recorded","state_changed":false})
    );
    send(
        &app,
        "POST",
        COMMANDS,
        &change("revoke", "revoke", "tenant.provision"),
    )
    .await;
    assert_eq!(
        send(&app, "POST", COMMANDS, &payload).await,
        json!({"outcome":"already_recorded","state_changed":true})
    );
    assert_eq!(
        send(&app, "GET", GRANTS, &Value::Null).await,
        json!({"grants":[],"next_after":null})
    );
    assert_eq!(
        send(
            &app,
            "POST",
            COMMANDS,
            &change("absent", "revoke", "tenant.provision")
        )
        .await,
        json!({"outcome":"recorded","state_changed":false})
    );
    for (path, payload) in [
        (COMMANDS, change("grant-a", "grant", "identity.manage")),
        (COMMANDS, change("grant-a", "revoke", "tenant.provision")),
        (
            "/api/v1/admin/actors/operator-a/platform-access-changes",
            change("grant-a", "grant", "tenant.provision"),
        ),
    ] {
        let response = app
            .clone()
            .oneshot(request("POST", path, &payload))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        body(response).await;
    }
    assert_eq!(audit_count(&pool).await, 4);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn final_active_administrator_survives_then_authority_can_be_delegated(pool: PgPool) {
    let app = setup(&pool).await;
    let own = "/api/v1/admin/actors/operator-a/platform-access-changes";
    sqlx::query("UPDATE identity.actors SET is_active=false WHERE actor_id='actor-b'")
        .execute(&pool)
        .await
        .unwrap();
    send(
        &app,
        "POST",
        COMMANDS,
        &change("delegate", "grant", "platform.access.manage"),
    )
    .await;
    let payload = change("retire-self", "revoke", "platform.access.manage");
    let response = app
        .clone()
        .oneshot(request("POST", own, &payload))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(audit_count(&pool).await, 1);
    sqlx::query("UPDATE identity.actors SET is_active=true WHERE actor_id='actor-b'")
        .execute(&pool)
        .await
        .unwrap();
    send(&app, "POST", own, &payload).await;
    // Both reads and exact retries need new current authority after self-revocation.
    for (method, path) in [("GET", GRANTS), ("GET", AUDIT), ("POST", own)] {
        let response = app
            .clone()
            .oneshot(request(method, path, &payload))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    assert_eq!(audit_count(&pool).await, 2);
    // The new administrator can authenticate using the actor's existing exact mapping.
    let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workforce", "subject-actor-b")
        .unwrap();
    let auth = CallerAuthenticationService::new(
        crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
            config.bearer_identity().unwrap().clone(),
        ),
        PostgresActorIdentityResolver::new(pool.clone()),
    );
    let delegated =
        crate::protected_application_router(pool.clone(), auth, MutationGuard::Workload);
    // Reusing another administrator's command ID conflicts, even with identical business fields.
    let response = delegated
        .clone()
        .oneshot(request("POST", own, &payload))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    send(
        &delegated,
        "POST",
        own,
        &change("restore", "grant", "platform.access.manage"),
    )
    .await;
    assert_eq!(audit_count(&pool).await, 3);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn missing_targets_invalid_bodies_and_audit_failures_are_atomic_and_redacted(pool: PgPool) {
    let app = setup(&pool).await;
    for kind in ["grant", "revoke"] {
        let response = app
            .clone()
            .oneshot(request(
                "POST",
                "/api/v1/admin/actors/missing/platform-access-changes",
                &change("missing", kind, "tenant.provision"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let valid = change("reject-audit", "grant", "tenant.provision");
    for (field, value) in [
        ("administrator_actor_id", json!("forged")),
        ("target_actor_id", json!("forged")),
        ("recorded_at_ms", json!(1)),
        ("action", json!("metadata.read")),
        ("action", json!("Identity.Manage")),
        ("kind", json!("Grant")),
        ("change_id", json!("")),
        ("change_id", json!("x".repeat(20_000))),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        let response = app
            .clone()
            .oneshot(request("POST", COMMANDS, &invalid))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        body(response).await;
    }
    sqlx::query("ALTER TABLE identity.platform_access_changes ADD CONSTRAINT test_reject_audit CHECK (change_id <> 'reject-audit')").execute(&pool).await.unwrap();
    let response = app
        .clone()
        .oneshot(request("POST", COMMANDS, &valid))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let text = body(response).await.to_string();
    for private in ["test_reject_audit", "23514", "INSERT", TOKEN] {
        assert!(!text.contains(private));
    }
    assert_eq!(audit_count(&pool).await, 0);
    assert_eq!(
        send(&app, "GET", GRANTS, &Value::Null).await["grants"],
        json!([])
    );
}

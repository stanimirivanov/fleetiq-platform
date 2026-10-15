//! Grant mutation, collision, rollback, and lockout behavior over the composed HTTP boundary.
use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_http_retries_keep_one_fact_and_never_restore_a_revoked_grant(pool: PgPool) {
    let app = setup(&pool).await;
    let payload = change("grant-a", "grant", "metadata.read");
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
            &change("noop", "grant", "metadata.read")
        )
        .await,
        json!({"outcome":"recorded","state_changed":false})
    );
    send(
        &app,
        "POST",
        COMMANDS,
        &change("revoke", "revoke", "metadata.read"),
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
            &change("absent", "revoke", "metadata.read")
        )
        .await,
        json!({"outcome":"recorded","state_changed":false})
    );
    for (path, payload) in [
        (COMMANDS, change("grant-a", "grant", "metadata.manage")),
        (COMMANDS, change("grant-a", "revoke", "metadata.read")),
        (
            "/api/v1/tenants/tenant-a/actors/operator-a/access-changes",
            change("grant-a", "grant", "metadata.read"),
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
async fn missing_targets_invalid_bodies_and_audit_failures_are_atomic_and_redacted(pool: PgPool) {
    let app = setup(&pool).await;
    for kind in ["grant", "revoke"] {
        let response = app
            .clone()
            .oneshot(request(
                "POST",
                "/api/v1/tenants/tenant-a/actors/missing/access-changes",
                &change("missing", kind, "metadata.read"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let valid = change("reject-audit", "grant", "metadata.read");
    for (field, value) in [
        ("administrator_actor_id", json!("forged")),
        ("target_actor_id", json!("forged")),
        ("tenant_id", json!("tenant-b")),
        ("recorded_at_ms", json!(1)),
        ("action", json!("platform.access.manage")),
        ("action", json!("Metadata.Read")),
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
    sqlx::query("ALTER TABLE identity.tenant_access_changes ADD CONSTRAINT test_reject_audit CHECK (change_id <> 'reject-audit')").execute(&pool).await.unwrap();
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

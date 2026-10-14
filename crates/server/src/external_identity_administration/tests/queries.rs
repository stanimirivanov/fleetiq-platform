use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn current_mapping_pages_preserve_exact_cursor_values_after_detach(pool: PgPool) {
    let app = setup(&pool).await;
    let response = app
        .clone()
        .oneshot(request("GET", MAPPINGS, &Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
    crate::contract_test_support::assert_published_example(
        &bytes,
        "/api/v1/admin/actors/{actor_id}/external-identities",
    );
    for (id, subject) in [("attach-a", " A+é "), ("attach-b", "b")] {
        send(
            &app,
            "POST",
            COMMANDS,
            &change(id, "attach", "recovery", subject),
        )
        .await;
    }
    let first = send(&app, "GET", &format!("{MAPPINGS}?limit=1"), &Value::Null).await;
    assert_eq!(
        first["identities"],
        json!([{"authority_id":"recovery","subject":" A+é "}])
    );
    let cursor = &first["next_after"];
    send(
        &app,
        "POST",
        COMMANDS,
        &change("detach-a", "detach", "recovery", " A+é "),
    )
    .await;
    let next = continuation(
        MAPPINGS,
        &[
            ("limit", "1"),
            (
                "after_authority_id",
                cursor["authority_id"].as_str().unwrap(),
            ),
            ("after_subject", cursor["subject"].as_str().unwrap()),
        ],
    );
    let next = send(&app, "GET", &next, &Value::Null).await;
    assert_eq!(
        next["identities"],
        json!([{"authority_id":"recovery","subject":"b"}])
    );
    let last = continuation(
        MAPPINGS,
        &[("after_authority_id", "recovery"), ("after_subject", "b")],
    );
    assert_eq!(
        send(&app, "GET", &last, &Value::Null).await["next_after"],
        Value::Null
    );
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/admin/actors/missing/external-identities",
            &Value::Null
        )
        .await,
        json!({"identities":[],"next_after":null})
    );
    let operator = send(
        &app,
        "GET",
        "/api/v1/admin/actors/operator-a/external-identities",
        &Value::Null,
    )
    .await;
    assert_eq!(
        operator["identities"],
        json!([{"authority_id":"workload","subject":"operator"}])
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn audit_pages_keep_original_administrator_subject_time_and_effect(pool: PgPool) {
    let app = setup(&pool).await;
    assert_eq!(
        send(&app, "GET", AUDIT, &Value::Null).await,
        json!({"changes":[],"next_before":null})
    );
    for (id, kind) in [("change-a", "attach"), ("change-b", "detach")] {
        send(
            &app,
            "POST",
            COMMANDS,
            &change(id, kind, "recovery", " Subject "),
        )
        .await;
    }
    let first = send(&app, "GET", &format!("{AUDIT}?limit=1"), &Value::Null).await;
    let entry = &first["changes"][0];
    assert_eq!(entry["change_id"], "change-b");
    assert_eq!(entry["administrator_actor_id"], "operator-a");
    assert_eq!(entry["target_actor_id"], "actor-b");
    assert_eq!(
        entry["external_identity"],
        json!({"authority_id":"recovery","subject":" Subject "})
    );
    assert_eq!(entry["state_changed"], true);
    let cursor = &first["next_before"];
    let path = continuation(
        AUDIT,
        &[
            ("limit", "1"),
            (
                "before_recorded_at_ms",
                &cursor["recorded_at_ms"].to_string(),
            ),
            ("before_change_id", cursor["change_id"].as_str().unwrap()),
        ],
    );
    let next = send(&app, "GET", &path, &Value::Null).await;
    assert_eq!(next["changes"][0]["change_id"], "change-a");
    assert_eq!(next["next_before"], Value::Null);
    assert_eq!(audit_count(&pool).await, 2);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn bounded_queries_and_unsupported_methods_return_redacted_noncacheable_problems(
    pool: PgPool,
) {
    let app = setup(&pool).await;
    for path in [
        format!("{MAPPINGS}?limit=0"),
        format!("{MAPPINGS}?limit=101"),
        format!("{MAPPINGS}?limit=1&limit=2"),
        format!("{MAPPINGS}?unknown=1"),
        format!("{MAPPINGS}?after_authority_id=recovery"),
        format!("{MAPPINGS}?after_subject=private-subject"),
        format!("{MAPPINGS}?after_authority_id=recovery&after_subject="),
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
        assert!(!body(response).await.to_string().contains("private-subject"));
    }
    for (method, path, allow) in [
        ("POST", MAPPINGS, "GET, HEAD"),
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
        assert_eq!(body(response).await["status"], 405);
    }
    let response = app
        .clone()
        .oneshot(request("HEAD", MAPPINGS, &Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert!(
        to_bytes(response.into_body(), 8192)
            .await
            .unwrap()
            .is_empty()
    );
    let config =
        crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0").unwrap();
    let unconfigured = crate::application_router(pool, &config).await.unwrap();
    assert_eq!(
        unconfigured
            .oneshot(request("GET", MAPPINGS, &Value::Null))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn every_operation_checks_current_exact_identity_authority_before_validation(pool: PgPool) {
    let app = setup(&pool).await;
    sqlx::query("DELETE FROM identity.platform_action_grants WHERE actor_id='operator-a' AND action='identity.manage'").execute(&pool).await.unwrap();
    for (method, path, payload) in [
        ("GET", format!("{MAPPINGS}?limit=0"), Value::Null),
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

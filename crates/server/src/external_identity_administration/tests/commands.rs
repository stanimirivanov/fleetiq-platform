use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_retries_preserve_exact_subject_and_never_reapply_old_mapping(pool: PgPool) {
    let app = setup(&pool).await;
    let payload = change("attach-a", "attach", "recovery", " User+Case/é ");
    let (a, b) = tokio::join!(
        app.clone().oneshot(request("POST", COMMANDS, &payload)),
        app.clone().oneshot(request("POST", COMMANDS, &payload)),
    );
    let mut results = vec![body(a.unwrap()).await, body(b.unwrap()).await];
    results.sort_by_key(|v| v["outcome"].as_str().unwrap().to_owned());
    assert_eq!(
        results,
        vec![
            json!({"outcome":"already_recorded","state_changed":true}),
            json!({"outcome":"recorded","state_changed":true})
        ]
    );
    assert_eq!(audit_count(&pool).await, 1);
    let list = send(&app, "GET", MAPPINGS, &Value::Null).await;
    assert!(
        list["identities"]
            .as_array()
            .unwrap()
            .contains(&json!({"authority_id":"recovery","subject":" User+Case/é "}))
    );
    assert_eq!(
        send(
            &app,
            "POST",
            COMMANDS,
            &change("attach-noop", "attach", "recovery", " User+Case/é ")
        )
        .await,
        json!({"outcome":"recorded","state_changed":false})
    );
    send(
        &app,
        "POST",
        COMMANDS,
        &change("detach-a", "detach", "recovery", " User+Case/é "),
    )
    .await;
    assert_eq!(
        send(&app, "POST", COMMANDS, &payload).await,
        json!({"outcome":"already_recorded","state_changed":true})
    );
    let list = send(&app, "GET", MAPPINGS, &Value::Null).await;
    assert_eq!(
        list["identities"],
        json!([{"authority_id":"workforce","subject":"subject-actor-b"}])
    );
    assert_eq!(
        send(
            &app,
            "POST",
            COMMANDS,
            &change("detach-noop", "detach", "recovery", " User+Case/é ")
        )
        .await,
        json!({"outcome":"recorded","state_changed":false})
    );
    assert_eq!(audit_count(&pool).await, 4);
    let response = app
        .oneshot(request(
            "POST",
            COMMANDS,
            &change("attach-a", "attach", "recovery", "user+case/é"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn replacement_is_explicit_case_sensitive_and_preserves_last_identity(pool: PgPool) {
    let app = setup(&pool).await;
    for (kind, authority, subject) in [
        ("detach", "workforce", "subject-actor-b"),
        ("attach", "workload", "operator"),
        ("detach", "workload", "operator"),
    ] {
        let response = app
            .clone()
            .oneshot(request(
                "POST",
                COMMANDS,
                &change("blocked", kind, authority, subject),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let problem = body(response).await.to_string();
        assert!(!problem.contains(subject));
    }
    assert_eq!(audit_count(&pool).await, 0);
    for (id, authority, subject) in [
        ("case-upper", "recovery", "User"),
        ("case-lower", "recovery", "user"),
        ("case-space", "recovery", " User "),
        ("authority-case", "Recovery", "User"),
    ] {
        send(
            &app,
            "POST",
            COMMANDS,
            &change(id, "attach", authority, subject),
        )
        .await;
    }
    let list = send(&app, "GET", MAPPINGS, &Value::Null).await;
    assert_eq!(list["identities"].as_array().unwrap().len(), 5);
    send(
        &app,
        "POST",
        COMMANDS,
        &change("retire-initial", "detach", "workforce", "subject-actor-b"),
    )
    .await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM identity.actor_provisionings WHERE actor_id='actor-b'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    for kind in ["attach", "detach"] {
        let response = app
            .clone()
            .oneshot(request(
                "POST",
                "/api/v1/admin/actors/missing/external-identity-changes",
                &change("missing", kind, "recovery", "new"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    assert_eq!(audit_count(&pool).await, 5);
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn invalid_payloads_and_failed_audit_never_change_mapping_or_echo_subject(pool: PgPool) {
    let app = setup(&pool).await;
    let secret_subject = "opaque-subject-not-for-diagnostics";
    let payload = change("reject-audit", "attach", "recovery", secret_subject);
    for extra in [
        "administrator_actor_id",
        "recorded_at_ms",
        "target_actor_id",
    ] {
        let mut forged = payload.clone();
        forged[extra] = json!("forged");
        let response = app
            .clone()
            .oneshot(request("POST", COMMANDS, &forged))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!body(response).await.to_string().contains(secret_subject));
    }
    for (field, value) in [
        ("subject", json!("")),
        ("subject", json!("   ")),
        ("subject", json!("a\u{0000}")),
        ("subject", json!("é".repeat(513))),
        ("authority_id", json!(" bad")),
    ] {
        let mut invalid = payload.clone();
        invalid["external_identity"][field] = value;
        assert_eq!(
            app.clone()
                .oneshot(request("POST", COMMANDS, &invalid))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let mut huge = payload.clone();
    huge["external_identity"]["subject"] = json!("x".repeat(20_000));
    assert_eq!(
        app.clone()
            .oneshot(request("POST", COMMANDS, &huge))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    sqlx::query("ALTER TABLE identity.external_identity_changes ADD CONSTRAINT test_reject_audit CHECK (change_id <> 'reject-audit')").execute(&pool).await.unwrap();
    let response = app
        .oneshot(request("POST", COMMANDS, &payload))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let problem = body(response).await.to_string();
    for private in [secret_subject, "test_reject_audit", "23514", "INSERT"] {
        assert!(!problem.contains(private));
    }
    assert_eq!(audit_count(&pool).await, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM identity.external_actor_identities WHERE authority_id='recovery'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
}

use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_provisioning_retries_have_one_winner_and_no_grants(pool: PgPool) {
    let app = setup(&pool).await;
    let payload = provision("provision-a", "actor-a");
    let (first, second) = tokio::join!(
        app.clone().oneshot(request("POST", ACTORS, &payload)),
        app.clone().oneshot(request("POST", ACTORS, &payload))
    );
    let statuses = [first.unwrap().status(), second.unwrap().status()];
    assert_eq!(
        statuses
            .iter()
            .filter(|s| **s == StatusCode::CREATED)
            .count(),
        1
    );
    assert_eq!(statuses.iter().filter(|s| **s == StatusCode::OK).count(), 1);
    let retry = app
        .clone()
        .oneshot(request("POST", ACTORS, &payload))
        .await
        .unwrap();
    assert_eq!(body(retry).await, json!({"outcome":"already_provisioned"}));
    let catalogue = app
        .clone()
        .oneshot(request("GET", ACTORS, &Value::Null))
        .await
        .unwrap();
    let catalogue_body = to_bytes(catalogue.into_body(), 8192).await.unwrap();
    crate::contract_test_support::assert_published_example(&catalogue_body, ACTORS);
    let changed = app
        .oneshot(request(
            "POST",
            ACTORS,
            &provision("provision-a", "actor-b"),
        ))
        .await
        .unwrap();
    assert_eq!(changed.status(), StatusCode::CONFLICT);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identity.actor_provisionings")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM identity.platform_action_grants WHERE actor_id='actor-a'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn lifecycle_retries_preserve_original_effect_and_do_not_reapply_state(pool: PgPool) {
    let app = setup(&pool).await;
    app.clone()
        .oneshot(request("POST", ACTORS, &provision("p-a", "actor-a")))
        .await
        .unwrap();
    let path = "/api/v1/admin/actors/actor-a/lifecycle";
    let payload = lifecycle("deactivate-a", "deactivated");
    let (first, second) = tokio::join!(
        app.clone().oneshot(request("POST", path, &payload)),
        app.clone().oneshot(request("POST", path, &payload))
    );
    let mut outcomes = vec![body(first.unwrap()).await, body(second.unwrap()).await];
    outcomes.sort_by_key(|value| value["outcome"].as_str().unwrap().to_owned());
    assert_eq!(
        outcomes,
        vec![
            json!({"outcome":"already_recorded","state_changed":true}),
            json!({"outcome":"recorded","state_changed":true})
        ]
    );
    assert_eq!(
        body(
            app.clone()
                .oneshot(request("POST", path, &lifecycle("activate-a", "active")))
                .await
                .unwrap()
        )
        .await,
        json!({"outcome":"recorded","state_changed":true})
    );
    let retry = app
        .clone()
        .oneshot(request("POST", path, &payload))
        .await
        .unwrap();
    assert_eq!(
        body(retry).await,
        json!({"outcome":"already_recorded","state_changed":true})
    );
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT is_active FROM identity.actors WHERE actor_id='actor-a'"
        )
        .fetch_one(&pool)
        .await
        .unwrap()
    );
    assert_eq!(
        body(
            app.clone()
                .oneshot(request("POST", path, &lifecycle("activate-noop", "active")))
                .await
                .unwrap()
        )
        .await,
        json!({"outcome":"recorded","state_changed":false})
    );
    assert_eq!(
        app.clone()
            .oneshot(request("POST", path, &lifecycle("deactivate-a", "active")))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.oneshot(request(
            "POST",
            "/api/v1/admin/actors/missing/lifecycle",
            &lifecycle("missing", "active")
        ))
        .await
        .unwrap()
        .status(),
        StatusCode::NOT_FOUND
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn lockout_and_audit_write_failure_leave_state_and_history_unchanged(pool: PgPool) {
    let app = setup(&pool).await;
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/admin/actors/operator-a/lifecycle",
            &lifecycle("last-admin", "deactivated"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    app.clone()
        .oneshot(request("POST", ACTORS, &provision("p-a", "actor-a")))
        .await
        .unwrap();
    sqlx::query("ALTER TABLE identity.actor_lifecycle_changes ADD CONSTRAINT test_reject_audit CHECK (change_id <> 'reject-audit')").execute(&pool).await.unwrap();
    let response = app
        .oneshot(request(
            "POST",
            "/api/v1/admin/actors/actor-a/lifecycle",
            &lifecycle("reject-audit", "deactivated"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        !body(response)
            .await
            .to_string()
            .contains("test_reject_audit")
    );
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT is_active FROM identity.actors WHERE actor_id='actor-a'"
        )
        .fetch_one(&pool)
        .await
        .unwrap()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identity.actor_lifecycle_changes")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn provisioning_mapping_collision_rolls_back_actor_and_audit(pool: PgPool) {
    let app = setup(&pool).await;
    sqlx::query("INSERT INTO identity.external_actor_identities (authority_id,subject,actor_id) VALUES ('workforce','subject-actor-a','operator-a')").execute(&pool).await.unwrap();
    let response = app
        .oneshot(request("POST", ACTORS, &provision("p-a", "actor-a")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let counts=sqlx::query_as::<_,(i64,i64)>("SELECT (SELECT count(*) FROM identity.actors),(SELECT count(*) FROM identity.actor_provisionings)").fetch_one(&pool).await.unwrap();
    assert_eq!(counts, (1, 0));
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn authenticated_mutations_reject_unknown_fields_invalid_subjects_and_oversized_bodies(
    pool: PgPool,
) {
    let app = setup(&pool).await;
    let mut forged = provision("p-a", "actor-a");
    forged["provisioner_actor_id"] = json!("someone-else");
    let mut invalid_subject = provision("p-a", "actor-a");
    invalid_subject["external_identity"]["subject"] = json!("\u{0000}");
    for payload in [
        forged,
        invalid_subject,
        provision("", "actor-a"),
        provision("p-a", "x".repeat(20_000).as_str()),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request("POST", ACTORS, &payload))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identity.actor_provisionings")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}

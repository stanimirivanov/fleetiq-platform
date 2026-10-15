use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_provisioning_is_atomic_and_retry_does_not_restore_state(pool: PgPool) {
    let app = setup(&pool).await;
    let payload = provision("provision-a", "tenant-a");
    let (a, b) = tokio::join!(
        app.clone().oneshot(request("POST", TENANTS, &payload)),
        app.clone().oneshot(request("POST", TENANTS, &payload))
    );
    let statuses = [a.unwrap().status(), b.unwrap().status()];
    assert_eq!(
        statuses
            .iter()
            .filter(|s| **s == StatusCode::CREATED)
            .count(),
        1
    );
    assert_eq!(statuses.iter().filter(|s| **s == StatusCode::OK).count(), 1);
    let grants = sqlx::query_as::<_, (String, String)>(
        "SELECT actor_id,action FROM identity.tenant_action_grants ORDER BY action",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        grants,
        ["access.manage", "metadata.manage", "metadata.read"]
            .map(|a| ("admin-a".to_owned(), a.to_owned()))
    );
    let audit=sqlx::query_as::<_,(String,String,i64)>("SELECT provisioner_actor_id,initial_administrator_actor_id,recorded_at_ms FROM identity.tenant_provisionings").fetch_one(&pool).await.unwrap();
    assert_eq!((&*audit.0, &*audit.1), ("operator-a", "admin-a"));
    assert!(audit.2 > 100);
    // Later administration is never replayed by the original provisioning command.
    assert_eq!(
        app.clone()
            .oneshot(request(
                "POST",
                LIFECYCLE,
                &lifecycle("suspend-a", "suspended")
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        body(
            app.clone()
                .oneshot(request("POST", TENANTS, &payload))
                .await
                .unwrap()
        )
        .await,
        json!({"outcome":"already_provisioned"})
    );
    assert!(
        !sqlx::query_scalar::<_, bool>(
            "SELECT is_active FROM metadata.tenants WHERE tenant_id='tenant-a'"
        )
        .fetch_one(&pool)
        .await
        .unwrap()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identity.tenant_action_grants")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT recorded_at_ms FROM identity.tenant_provisionings")
            .fetch_one(&pool)
            .await
            .unwrap(),
        audit.2
    );
    for changed in [
        provision("provision-a", "tenant-b"),
        provision("different-id", "tenant-a"),
        json!({"provisioning_id":"provision-a","tenant_id":"tenant-a","initial_administrator_actor_id":"admin-b"}),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request("POST", TENANTS, &changed))
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn lifecycle_retries_return_original_effect_after_later_transitions(pool: PgPool) {
    let app = setup(&pool).await;
    create(&app, "tenant-a").await;
    let payload = lifecycle("suspend-a", "suspended");
    let (a, b) = tokio::join!(
        app.clone().oneshot(request("POST", LIFECYCLE, &payload)),
        app.clone().oneshot(request("POST", LIFECYCLE, &payload))
    );
    let mut outcomes = vec![body(a.unwrap()).await, body(b.unwrap()).await];
    outcomes.sort_by_key(|v| v["outcome"].as_str().unwrap().to_owned());
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
                .oneshot(request(
                    "POST",
                    LIFECYCLE,
                    &lifecycle("activate-a", "active")
                ))
                .await
                .unwrap()
        )
        .await,
        json!({"outcome":"recorded","state_changed":true})
    );
    assert_eq!(
        body(
            app.clone()
                .oneshot(request("POST", LIFECYCLE, &payload))
                .await
                .unwrap()
        )
        .await,
        json!({"outcome":"already_recorded","state_changed":true})
    );
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT is_active FROM metadata.tenants WHERE tenant_id='tenant-a'"
        )
        .fetch_one(&pool)
        .await
        .unwrap()
    );
    assert_eq!(
        body(
            app.clone()
                .oneshot(request(
                    "POST",
                    LIFECYCLE,
                    &lifecycle("activate-noop", "active")
                ))
                .await
                .unwrap()
        )
        .await,
        json!({"outcome":"recorded","state_changed":false})
    );
    assert_eq!(
        app.clone()
            .oneshot(request(
                "POST",
                LIFECYCLE,
                &lifecycle("suspend-a", "active")
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.oneshot(request(
            "POST",
            "/api/v1/admin/tenants/missing/lifecycle",
            &lifecycle("missing", "suspended")
        ))
        .await
        .unwrap()
        .status(),
        StatusCode::NOT_FOUND
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn suspension_controls_operator_discovery_and_fresh_tenant_reads(pool: PgPool) {
    let app = setup(&pool).await;
    create(&app, "tenant-a").await;
    let admin = router(&pool, "administrator");
    let assets = "/api/v1/tenants/tenant-a/assets";
    assert_eq!(
        admin
            .clone()
            .oneshot(request("GET", assets, &Value::Null))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        body(
            admin
                .clone()
                .oneshot(request("GET", "/api/v1/me/tenants", &Value::Null))
                .await
                .unwrap()
        )
        .await["tenants"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        app.clone()
            .oneshot(request(
                "POST",
                LIFECYCLE,
                &lifecycle("suspend-a", "suspended")
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        admin
            .clone()
            .oneshot(request("GET", assets, &Value::Null))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        body(
            admin
                .clone()
                .oneshot(request("GET", "/api/v1/me/tenants", &Value::Null))
                .await
                .unwrap()
        )
        .await["tenants"],
        json!([])
    );
    assert_eq!(
        app.clone()
            .oneshot(request(
                "POST",
                LIFECYCLE,
                &lifecycle("activate-a", "active")
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        admin
            .oneshot(request("GET", assets, &Value::Null))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.oneshot(request("GET", assets, &Value::Null))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn missing_admin_and_database_failure_roll_back_with_safe_problems(pool: PgPool) {
    let app = setup(&pool).await;
    let missing = json!({"provisioning_id":"missing","tenant_id":"tenant-a","initial_administrator_actor_id":"missing"});
    let response = app
        .clone()
        .oneshot(request("POST", TENANTS, &missing))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(body(response).await["title"], "Resource not found");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identity.tenant_provisionings")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    sqlx::query("ALTER TABLE identity.tenant_action_grants ADD CONSTRAINT private_fixture CHECK (actor_id <> 'admin-a')").execute(&pool).await.unwrap();
    let response = app
        .clone()
        .oneshot(request("POST", TENANTS, &provision("failed", "tenant-a")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!body(response).await.to_string().contains("private_fixture"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM metadata.tenants")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identity.tenant_provisionings")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn lifecycle_audit_failure_rolls_back_state_and_redacts_storage_details(pool: PgPool) {
    let app = setup(&pool).await;
    create(&app, "tenant-a").await;
    sqlx::query("ALTER TABLE identity.tenant_lifecycle_changes ADD CONSTRAINT private_lifecycle_fixture CHECK (change_id <> 'blocked')").execute(&pool).await.unwrap();
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            LIFECYCLE,
            &lifecycle("blocked", "suspended"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        !body(response)
            .await
            .to_string()
            .contains("private_lifecycle_fixture")
    );
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT is_active FROM metadata.tenants WHERE tenant_id='tenant-a'"
        )
        .fetch_one(&pool)
        .await
        .unwrap()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identity.tenant_lifecycle_changes")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}

//! Tenant ownership and fresh authority are checked before request details.
use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn tenant_scope_applies_to_commands_catalogues_audit_and_change_ids(pool: PgPool) {
    let app = setup(&pool).await;
    let b_command = COMMANDS.replace("tenant-a", "tenant-b");
    let b_grants = GRANTS.replace("tenant-a", "tenant-b");
    let b_audit = AUDIT.replace("tenant-a", "tenant-b");
    let payload = change("shared-id", "grant", "metadata.read");
    send(&app, "POST", COMMANDS, &payload).await;
    for (method, path) in [("POST", &b_command), ("GET", &b_grants), ("GET", &b_audit)] {
        let response = app
            .clone()
            .oneshot(request(method, path, &payload))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        body(response).await;
    }
    // A second explicit tenant grant is needed even for a platform administrator.
    sqlx::query("INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-b','operator-a','access.manage')").execute(&pool).await.unwrap();
    assert_eq!(
        send(&app, "GET", &b_grants, &Value::Null).await["grants"],
        json!([])
    );
    send(&app, "POST", &b_command, &payload).await;
    assert_eq!(audit_count(&pool).await, 2);
    for (path, tenant) in [(AUDIT, "tenant-a"), (&b_audit, "tenant-b")] {
        let page = send(&app, "GET", path, &Value::Null).await;
        assert_eq!(page["changes"].as_array().unwrap().len(), 1);
        assert_eq!(page["changes"][0]["tenant_id"], tenant);
    }
    send(
        &app,
        "POST",
        &b_command,
        &change("remove", "revoke", "metadata.read"),
    )
    .await;
    assert_eq!(
        send(&app, "GET", &b_grants, &Value::Null).await["grants"],
        json!([])
    );
    assert_eq!(
        send(&app, "GET", GRANTS, &Value::Null).await["grants"],
        json!([{"action":"metadata.read"}])
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn exact_current_tenant_authority_precedes_validation_and_platform_grants_do_not_substitute(
    pool: PgPool,
) {
    let app = setup(&pool).await;
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE actor_id='operator-a' AND action='access.manage'").execute(&pool).await.unwrap();
    for (method, path) in [
        ("GET", format!("{GRANTS}?limit=0")),
        ("GET", AUDIT.into()),
        ("POST", COMMANDS.into()),
    ] {
        let response = app
            .clone()
            .oneshot(request(method, &path, &json!({})))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        body(response).await;
        let mut anonymous = request(method, &path, &json!({}));
        anonymous.headers_mut().remove(header::AUTHORIZATION);
        let response = app.clone().oneshot(anonymous).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().contains_key(header::WWW_AUTHENTICATE));
        body(response).await;
    }
    assert_eq!(audit_count(&pool).await, 0);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn self_revocation_and_suspension_deny_fresh_requests_without_erasing_evidence(pool: PgPool) {
    let app = setup(&pool).await;
    let own = COMMANDS.replace("actor-b", "operator-a");
    let payload = change("retire", "revoke", "access.manage");
    send(&app, "POST", &own, &payload).await;
    for (method, path) in [("POST", own.as_str()), ("GET", GRANTS), ("GET", AUDIT)] {
        let response = app
            .clone()
            .oneshot(request(method, path, &payload))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    // Existing tenant policy allows a final self-revocation. No platform bypass is introduced.
    assert_eq!(audit_count(&pool).await, 1);
    sqlx::query("INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-a','operator-a','access.manage')").execute(&pool).await.unwrap();
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    for (method, path) in [("POST", COMMANDS), ("GET", GRANTS), ("GET", AUDIT)] {
        let response = app
            .clone()
            .oneshot(request(
                method,
                path,
                &change("blocked", "grant", "metadata.read"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        body(response).await;
    }
    assert_eq!(audit_count(&pool).await, 1);
    sqlx::query("UPDATE metadata.tenants SET is_active=true WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        send(&app, "GET", AUDIT, &Value::Null).await["changes"][0]["change_id"],
        "retire"
    );
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn grants_change_live_access_and_retry_identity_includes_the_administrator(pool: PgPool) {
    let app = setup(&pool).await;
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
    let target = crate::protected_application_router(pool.clone(), auth, MutationGuard::Workload);
    let assets = "/api/v1/tenants/tenant-a/assets";
    assert_eq!(
        target
            .clone()
            .oneshot(request("GET", assets, &Value::Null))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let payload = change("live-reader", "grant", "metadata.read");
    send(&app, "POST", COMMANDS, &payload).await;
    assert_eq!(
        target
            .clone()
            .oneshot(request("GET", assets, &Value::Null))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    send(
        &app,
        "POST",
        COMMANDS,
        &change("delegate", "grant", "access.manage"),
    )
    .await;
    let response = target
        .clone()
        .oneshot(request("POST", COMMANDS, &payload))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    send(
        &app,
        "POST",
        COMMANDS,
        &change("remove-reader", "revoke", "metadata.read"),
    )
    .await;
    assert_eq!(
        target
            .clone()
            .oneshot(request("GET", assets, &Value::Null))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    // Retained grants remain visible to an administrator even when the target is inactive.
    sqlx::query("UPDATE identity.actors SET is_active=false WHERE actor_id='actor-b'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        send(&app, "GET", GRANTS, &Value::Null).await["grants"],
        json!([{"action":"access.manage"}])
    );
    assert_eq!(
        target
            .oneshot(request("GET", GRANTS, &Value::Null))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(audit_count(&pool).await, 3);
}

//! Metadata actions, namespace routing and tenant ownership remain independent.
use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn exact_read_and_manage_grants_do_not_imply_one_another(pool: PgPool) {
    let app = setup(&pool).await;
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='metadata.manage'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO identity.platform_action_grants (actor_id,action) VALUES ('operator-a','identity.manage')").execute(&pool).await.unwrap();
    for (resource, _) in definitions() {
        let path = format!("{ROOT}/{resource}");
        send(&app, "POST", &path, &json!({}), StatusCode::FORBIDDEN).await;
        send(&app, "GET", &path, &Value::Null, StatusCode::OK).await;
        let mut anonymous = request("POST", &path, &json!({}));
        anonymous.headers_mut().remove(header::AUTHORIZATION);
        let response = app.clone().oneshot(anonymous).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        body(response).await;
    }
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='metadata.read'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-a','operator-a','metadata.manage')").execute(&pool).await.unwrap();
    for (resource, payload) in definitions() {
        let path = format!("{ROOT}/{resource}");
        publish(&app, resource, &payload).await;
        for suffix in ["?limit=0", "/id/versions?limit=0"] {
            send(
                &app,
                "GET",
                &format!("{path}{suffix}"),
                &Value::Null,
                StatusCode::FORBIDDEN,
            )
            .await;
        }
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn equal_keys_in_other_tenants_stay_independent_and_suspension_is_fresh(pool: PgPool) {
    let app = setup(&pool).await;
    for (resource, payload) in definitions() {
        publish(&app, resource, &payload).await;
        let foreign = format!("/api/v1/tenants/tenant-b/{resource}");
        send(&app, "POST", &foreign, &payload, StatusCode::FORBIDDEN).await;
        send(&app, "GET", &foreign, &Value::Null, StatusCode::FORBIDDEN).await;
    }
    sqlx::query("INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-b','operator-a','metadata.read'),('tenant-b','operator-a','metadata.manage')").execute(&pool).await.unwrap();
    for (resource, mut payload) in definitions() {
        let foreign = format!("/api/v1/tenants/tenant-b/{resource}");
        assert_eq!(
            send(&app, "GET", &foreign, &Value::Null, StatusCode::OK).await["definitions"],
            json!([])
        );
        payload["name"] = json!("Other tenant");
        send(&app, "POST", &foreign, &payload, StatusCode::CREATED).await;
        assert_eq!(
            send(&app, "GET", &foreign, &Value::Null, StatusCode::OK).await["definitions"][0]["name"],
            "Other tenant"
        );
        assert_ne!(
            send(
                &app,
                "GET",
                &format!("{ROOT}/{resource}"),
                &Value::Null,
                StatusCode::OK
            )
            .await["definitions"][0]["name"],
            "Other tenant"
        );
    }
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-b'")
        .execute(&pool)
        .await
        .unwrap();
    for (resource, payload) in definitions() {
        let foreign = format!("/api/v1/tenants/tenant-b/{resource}");
        for (method, uri) in [
            ("GET", foreign.clone()),
            ("POST", foreign.clone()),
            ("GET", format!("{foreign}/id/versions")),
        ] {
            send(&app, method, &uri, &payload, StatusCode::FORBIDDEN).await;
        }
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn storage_errors_are_redacted_and_unconfigured_routes_are_absent(pool: PgPool) {
    let app = setup(&pool).await;
    sqlx::query("ALTER TABLE metadata.property_definitions ADD CONSTRAINT test_reject_publication CHECK (display_name <> 'Temperature')").execute(&pool).await.unwrap();
    let error = send(
        &app,
        "POST",
        &format!("{ROOT}/properties"),
        &definitions()[1].1,
        StatusCode::INTERNAL_SERVER_ERROR,
    )
    .await
    .to_string();
    for private in ["test_reject_publication", "23514", "INSERT", TOKEN] {
        assert!(!error.contains(private));
    }
    assert_eq!(
        send(
            &app,
            "GET",
            &format!("{ROOT}/properties"),
            &Value::Null,
            StatusCode::OK
        )
        .await["definitions"],
        json!([])
    );
    let config =
        crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0").unwrap();
    let unconfigured = crate::application_router(pool, &config).await.unwrap();
    for (resource, payload) in definitions() {
        for (method, suffix) in [("GET", ""), ("POST", ""), ("GET", "/id/versions")] {
            assert_eq!(
                unconfigured
                    .clone()
                    .oneshot(request(
                        method,
                        &format!("{ROOT}/{resource}{suffix}"),
                        &payload
                    ))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::NOT_FOUND
            );
        }
    }
}

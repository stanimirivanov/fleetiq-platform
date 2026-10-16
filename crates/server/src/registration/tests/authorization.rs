use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn exact_current_manage_authority_precedes_body_errors_and_does_not_imply_read(pool: PgPool) {
    let app = setup(&pool).await;
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='metadata.manage'")
        .execute(&pool)
        .await
        .unwrap();
    for path in [
        ROOT.to_owned(),
        format!("{ROOT}/asset-a/device"),
        "/api/v1/tenants/tenant-b/assets".to_owned(),
        "/api/v1/tenants/missing/assets".to_owned(),
    ] {
        send(
            &app,
            "POST",
            &path,
            &json!({"invalid":true}),
            StatusCode::FORBIDDEN,
        )
        .await;
    }
    let mut anonymous = request("POST", ROOT, &asset());
    anonymous.headers_mut().remove(header::AUTHORIZATION);
    assert_eq!(
        app.clone().oneshot(anonymous).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    sqlx::raw_sql("INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-a','operator-a','metadata.manage');
        DELETE FROM identity.tenant_action_grants WHERE action='metadata.read';").execute(&pool).await.unwrap();
    send(&app, "POST", ROOT, &asset(), StatusCode::CREATED).await;
    send(
        &app,
        "GET",
        &format!("{ROOT}/asset-a"),
        &Value::Null,
        StatusCode::FORBIDDEN,
    )
    .await;
    sqlx::query("UPDATE metadata.tenants SET is_active=FALSE WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    send(
        &app,
        "POST",
        &format!("{ROOT}/asset-a/device"),
        &device(),
        StatusCode::FORBIDDEN,
    )
    .await;
    assert_eq!(audit_count(&pool).await, 1);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn storage_failure_is_neither_a_clean_registration_nor_a_database_detail_leak(pool: PgPool) {
    let app = setup(&pool).await;
    sqlx::query("DROP TABLE metadata.registration_records")
        .execute(&pool)
        .await
        .unwrap();
    let response = send(
        &app,
        "POST",
        ROOT,
        &asset(),
        StatusCode::INTERNAL_SERVER_ERROR,
    )
    .await;
    assert_eq!(response["title"], "Internal server error");
    assert!(!response.to_string().contains("registration_records"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM metadata.assets")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}

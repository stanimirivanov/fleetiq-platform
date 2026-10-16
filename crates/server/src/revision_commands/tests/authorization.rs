use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn exact_live_authority_precedes_input_validation_and_read_permission_is_independent(
    pool: PgPool,
) {
    let app = setup(&pool).await;
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='metadata.manage'")
        .execute(&pool)
        .await
        .unwrap();
    for binding in [false, true] {
        decode(
            app.clone()
                .oneshot(request("POST", &path(binding, "bad"), "malformed"))
                .await
                .unwrap(),
            StatusCode::FORBIDDEN,
        )
        .await;
        let mut missing = request("POST", &path(binding, "1"), "{}");
        missing.headers_mut().remove(header::AUTHORIZATION);
        decode(
            app.clone().oneshot(missing).await.unwrap(),
            StatusCode::UNAUTHORIZED,
        )
        .await;
    }
    sqlx::query("INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-a','operator-a','metadata.manage')").execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='metadata.read'")
        .execute(&pool)
        .await
        .unwrap();
    for binding in [false, true] {
        send(
            &app,
            "POST",
            &path(binding, "1"),
            &payload(binding),
            StatusCode::CREATED,
        )
        .await;
        send(
            &app,
            "GET",
            &path(binding, "1"),
            &json!({}),
            StatusCode::FORBIDDEN,
        )
        .await;
        send(
            &app,
            "POST",
            &path(binding, "1").replace("tenant-a", "tenant-b"),
            &payload(binding),
            StatusCode::FORBIDDEN,
        )
        .await;
    }
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    for binding in [false, true] {
        send(
            &app,
            "POST",
            &path(binding, "2"),
            &payload(binding),
            StatusCode::FORBIDDEN,
        )
        .await;
    }
    assert_eq!(count(&pool).await, 2);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn actor_evidence_failure_rolls_back_fact_head_and_hides_storage_details(pool: PgPool) {
    let app = setup(&pool).await;
    for (binding, table) in [
        (false, "relationship_revision_records"),
        (true, "signal_binding_revision_records"),
    ] {
        sqlx::query(if binding {
            "DROP TABLE metadata.signal_binding_revision_records"
        } else {
            "DROP TABLE metadata.relationship_revision_records"
        })
        .execute(&pool)
        .await
        .unwrap();
        let body = send(
            &app,
            "POST",
            &path(binding, "1"),
            &payload(binding),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .await;
        assert_eq!(body["title"], "Internal server error");
        assert!(!body.to_string().contains(table));

        assert_eq!(
            sqlx::query_scalar::<_, i64>(if binding {
                "SELECT count(*) FROM metadata.signal_bindings"
            } else {
                "SELECT count(*) FROM metadata.asset_relationships"
            })
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn foreign_only_targets_and_wrong_property_membership_cannot_satisfy_references(
    pool: PgPool,
) {
    let app = setup(&pool).await;
    sqlx::query("INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name) VALUES ('tenant-b','foreign-only','equipment',1,'Foreign')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind) VALUES ('tenant-a','unsupported',1,'Unsupported','decimal')").execute(&pool).await.unwrap();
    for binding in [false, true] {
        let mut bad = payload(binding);
        if binding {
            bad["target"]["asset_id"] = json!("foreign-only");
        } else {
            bad["target_asset_id"] = json!("foreign-only");
        }
        send(
            &app,
            "POST",
            &path(binding, "1"),
            &bad,
            StatusCode::NOT_FOUND,
        )
        .await;
    }
    let mut bad = payload(true);
    bad["target"]["property"]["id"] = json!("unsupported");
    send(&app, "POST", &path(true, "1"), &bad, StatusCode::NOT_FOUND).await;
    assert_eq!(count(&pool).await, 0);
}

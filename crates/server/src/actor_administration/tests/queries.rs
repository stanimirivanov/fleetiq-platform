use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn catalogue_and_audit_use_bounded_deterministic_pages(pool: PgPool) {
    let app = setup(&pool).await;
    assert_eq!(
        body(
            app.clone()
                .oneshot(request("GET", AUDIT, &Value::Null))
                .await
                .unwrap()
        )
        .await,
        json!({"changes":[],"next_before":null})
    );
    for actor in ["actor-a", "actor-b"] {
        app.clone()
            .oneshot(request("POST", ACTORS, &provision(actor, actor)))
            .await
            .unwrap();
    }
    let first = body(
        app.clone()
            .oneshot(request("GET", &format!("{ACTORS}?limit=1"), &Value::Null))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        first,
        json!({"actors":[{"actor_id":"actor-a","state":"active"}],"next_after":"actor-a"})
    );
    let next = body(
        app.clone()
            .oneshot(request(
                "GET",
                &format!("{ACTORS}?limit=1&after=actor-a"),
                &Value::Null,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(next["actors"][0]["actor_id"], "actor-b");
    for (id, state) in [("change-a", "deactivated"), ("change-b", "active")] {
        app.clone()
            .oneshot(request(
                "POST",
                "/api/v1/admin/actors/actor-a/lifecycle",
                &lifecycle(id, state),
            ))
            .await
            .unwrap();
    }
    let first = body(
        app.clone()
            .oneshot(request("GET", &format!("{AUDIT}?limit=1"), &Value::Null))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(first["changes"][0]["change_id"], "change-b");
    let cursor = &first["next_before"];
    let path = format!(
        "{AUDIT}?limit=1&before_recorded_at_ms={}&before_change_id={}",
        cursor["recorded_at_ms"],
        cursor["change_id"].as_str().unwrap()
    );
    let next = body(
        app.clone()
            .oneshot(request("GET", &path, &Value::Null))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(next["changes"][0]["change_id"], "change-a");
    assert_eq!(next["next_before"], Value::Null);
    for path in [
        format!("{ACTORS}?limit=0"),
        format!("{ACTORS}?after="),
        format!("{ACTORS}?unknown=1"),
        format!("{AUDIT}?before_change_id=a"),
        format!("{AUDIT}?before_recorded_at_ms=1"),
        format!("{AUDIT}?limit=101"),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request("GET", &path, &Value::Null))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn all_operations_require_exact_current_identity_management_before_validation(pool: PgPool) {
    let app = setup(&pool).await;
    sqlx::query("DELETE FROM identity.platform_action_grants WHERE actor_id='operator-a' AND action='identity.manage'").execute(&pool).await.unwrap();
    // Bootstrap still has the other three platform actions: none substitutes for identity.manage.
    for (method, path, payload) in [
        ("GET", format!("{ACTORS}?limit=0"), Value::Null),
        ("GET", AUDIT.to_owned(), Value::Null),
        ("POST", ACTORS.to_owned(), json!({})),
        (
            "POST",
            "/api/v1/admin/actors/target/lifecycle".to_owned(),
            json!({}),
        ),
    ] {
        let denied = app
            .clone()
            .oneshot(request(method, &path, &payload))
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        let mut anonymous = request(method, &path, &payload);
        anonymous.headers_mut().remove(header::AUTHORIZATION);
        let response = app.clone().oneshot(anonymous).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().contains_key(header::WWW_AUTHENTICATE));
    }
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn unsupported_methods_return_no_store_and_exact_allow(pool: PgPool) {
    let app = setup(&pool).await;
    for (method, path, allow) in [
        ("DELETE", ACTORS, "GET, HEAD, POST"),
        ("GET", "/api/v1/admin/actors/actor-a/lifecycle", "POST"),
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
}

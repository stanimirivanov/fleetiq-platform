use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn commission_correct_exact_read_and_retry_preserve_old_revision_receipts(pool: PgPool) {
    let app = setup(&pool).await;
    for binding in [false, true] {
        let first = payload(binding);
        let created = send(
            &app,
            "POST",
            &path(binding, "1"),
            &first,
            StatusCode::CREATED,
        )
        .await;
        assert_eq!(created["outcome"], "appended");
        assert_eq!(created["recorded_by"], "operator-a");
        let recorded = created["recorded_at_ms"].clone();
        let old = send(&app, "GET", &path(binding, "1"), &json!({}), StatusCode::OK).await;
        assert_eq!(old["recorded_at_ms"], recorded);
        let mut corrected = first.clone();
        if binding {
            corrected["source"]["signal_id"] = json!("bus.corrected");
            corrected["target"]["asset_id"] = json!("asset-c");
        } else {
            corrected["target_asset_id"] = json!("asset-c");
        }
        corrected["effective_interval"]["end_ms"] = json!(50);
        send(
            &app,
            "POST",
            &path(binding, "2"),
            &corrected,
            StatusCode::CREATED,
        )
        .await;
        let repeated = send(&app, "POST", &path(binding, "0001"), &first, StatusCode::OK).await;
        assert_eq!(repeated["revision"], "1");
        assert_eq!(repeated["recorded_at_ms"], recorded);
        assert_eq!(repeated["outcome"], "already_appended");
        assert_eq!(
            send(&app, "GET", &path(binding, "1"), &json!({}), StatusCode::OK).await,
            old
        );
        send(
            &app,
            "POST",
            &path(binding, "1"),
            &corrected,
            StatusCode::CONFLICT,
        )
        .await;
        send(
            &app,
            "POST",
            &path(binding, "4"),
            &corrected,
            StatusCode::CONFLICT,
        )
        .await;
        send(
            &app,
            "GET",
            &path(binding, "3"),
            &json!({}),
            StatusCode::NOT_FOUND,
        )
        .await;
        let response = app
            .clone()
            .oneshot(request("DELETE", &path(binding, "1"), "{}"))
            .await
            .unwrap();
        let allow = response.headers()[header::ALLOW].to_str().unwrap();
        for method in ["GET", "HEAD", "POST"] {
            assert!(allow.split(',').any(|v| v.trim() == method));
        }
        decode(response, StatusCode::METHOD_NOT_ALLOWED).await;
    }
    assert_eq!(count(&pool).await, 4);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_identical_commands_share_original_evidence(pool: PgPool) {
    let app = setup(&pool).await;
    for binding in [false, true] {
        let request1 = request("POST", &path(binding, "1"), &payload(binding).to_string());
        let request2 = request("POST", &path(binding, "1"), &payload(binding).to_string());
        let (a, b) = tokio::join!(app.clone().oneshot(request1), app.clone().oneshot(request2));
        let (a, b) = (a.unwrap(), b.unwrap());
        let mut statuses = [a.status().as_u16(), b.status().as_u16()];
        statuses.sort_unstable();
        assert_eq!(statuses, [200, 201]);
        let status_a = a.status();
        let status_b = b.status();
        let a = decode(a, status_a).await;
        let b = decode(b, status_b).await;
        assert_eq!(a["recorded_at_ms"], b["recorded_at_ms"]);
        assert_eq!(a["recorded_by"], b["recorded_by"]);
    }
    assert_eq!(count(&pool).await, 2);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn invalid_json_interval_reference_and_body_bounds_do_not_write(pool: PgPool) {
    let app = setup(&pool).await;
    for binding in [false, true] {
        for revision in ["0", "-1", "1.0", "18446744073709551616"] {
            send(
                &app,
                "POST",
                &path(binding, revision),
                &payload(binding),
                StatusCode::BAD_REQUEST,
            )
            .await;
        }
        for field in [
            "tenant_id",
            "actor_id",
            "recorded_at_ms",
            "revision",
            "id",
            "unknown",
        ] {
            let mut bad = payload(binding);
            bad[field] = json!("forged");
            send(
                &app,
                "POST",
                &path(binding, "1"),
                &bad,
                StatusCode::BAD_REQUEST,
            )
            .await;
        }
        for end in [-1, 0] {
            let mut bad = payload(binding);
            bad["effective_interval"]["end_ms"] = json!(end);
            send(
                &app,
                "POST",
                &path(binding, "1"),
                &bad,
                StatusCode::BAD_REQUEST,
            )
            .await;
        }
        let mut bad = payload(binding);
        bad["effective_interval"]["extra"] = json!(0);
        send(
            &app,
            "POST",
            &path(binding, "1"),
            &bad,
            StatusCode::BAD_REQUEST,
        )
        .await;
        let text = payload(binding)
            .to_string()
            .replace("\"start_ms\":0", "\"start_ms\":0,\"start_ms\":1");
        let response = app
            .clone()
            .oneshot(request("POST", &path(binding, "1"), &text))
            .await
            .unwrap();
        decode(response, StatusCode::BAD_REQUEST).await;
        let mut missing = payload(binding);
        if binding {
            missing["source"]["endpoint_id"] = json!("missing");
        } else {
            missing["relationship_type"]["version"] = json!(2);
        }
        send(
            &app,
            "POST",
            &path(binding, "1"),
            &missing,
            StatusCode::NOT_FOUND,
        )
        .await;
        let mut text = payload(binding).to_string();
        text.push_str(&" ".repeat(32 * 1024 - text.len()));
        let mut oversized = text.clone();
        oversized.push(' ');
        decode(
            app.clone()
                .oneshot(request("POST", &path(binding, "1"), &oversized))
                .await
                .unwrap(),
            StatusCode::BAD_REQUEST,
        )
        .await;
        decode(
            app.clone()
                .oneshot(request("POST", &path(binding, "1"), &text))
                .await
                .unwrap(),
            StatusCode::CREATED,
        )
        .await;
    }
    assert_eq!(count(&pool).await, 2);
}

use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn registers_and_retries_asset_and_device_through_unchanged_reads(pool: PgPool) {
    let app = setup(&pool).await;
    for (path, payload) in [
        (ROOT.to_owned(), asset()),
        (format!("{ROOT}/asset-a/device"), device()),
    ] {
        let first = send(&app, "POST", &path, &payload, StatusCode::CREATED).await;
        assert_eq!(first["outcome"], "registered");
        assert_eq!(first["registered_by"], "operator-a");
        assert!(first["recorded_at_ms"].is_i64());
        let repeated = send(&app, "POST", &path, &payload, StatusCode::OK).await;
        assert_eq!(repeated["outcome"], "already_registered");
        assert_eq!(repeated["registered_by"], first["registered_by"]);
        assert_eq!(repeated["recorded_at_ms"], first["recorded_at_ms"]);
    }
    let mut expected = asset();
    expected["tenant_id"] = json!("tenant-a");
    assert_eq!(
        send(
            &app,
            "GET",
            &format!("{ROOT}/asset-a"),
            &Value::Null,
            StatusCode::OK
        )
        .await,
        expected
    );
    let expected =
        json!({"tenant_id":"tenant-a","asset_id":"asset-a","endpoints":device()["endpoints"]});
    let actual = send(
        &app,
        "GET",
        &format!("{ROOT}/asset-a/device"),
        &Value::Null,
        StatusCode::OK,
    )
    .await;
    assert_eq!(actual, expected);
    let spec: Value = serde_json::from_str(include_str!(
        "../../../../../contracts/http/dist/openapi.json"
    ))
    .unwrap();
    let example = &spec["components"]["responses"]["getCommunicationDevice-200"]["content"]["application/json"]
        ["example"];
    assert_eq!(&actual, example);
    assert_eq!(audit_count(&pool).await, 2);
    let response = app
        .oneshot(request(
            "DELETE",
            &format!("{ROOT}/asset-a/device"),
            &Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    let mut allowed: Vec<_> = response.headers()[header::ALLOW]
        .to_str()
        .unwrap()
        .split(',')
        .map(str::trim)
        .collect();
    allowed.sort_unstable();
    assert_eq!(allowed, ["GET", "HEAD", "POST"]);
    body(response).await;
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn missing_references_and_collisions_are_redacted_and_atomic(pool: PgPool) {
    let app = setup(&pool).await;
    let mut missing = asset();
    missing["asset_type"]["version"] = json!(2);
    send(&app, "POST", ROOT, &missing, StatusCode::NOT_FOUND).await;
    send(
        &app,
        "POST",
        &format!("{ROOT}/missing/device"),
        &device(),
        StatusCode::NOT_FOUND,
    )
    .await;
    assert_eq!(audit_count(&pool).await, 0);
    send(&app, "POST", ROOT, &asset(), StatusCode::CREATED).await;
    let mut changed = asset();
    changed["name"] = json!("Different");
    let conflict = send(&app, "POST", ROOT, &changed, StatusCode::CONFLICT).await;
    assert_eq!(conflict["title"], "Registration identity is occupied");
    changed["id"] = json!("other");
    send(&app, "POST", ROOT, &changed, StatusCode::CONFLICT).await;
    assert_eq!(audit_count(&pool).await, 1);
    sqlx::query("INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name) VALUES ('tenant-b','foreign','equipment',1,'Foreign')").execute(&pool).await.unwrap();
    send(
        &app,
        "POST",
        &format!("{ROOT}/foreign/device"),
        &device(),
        StatusCode::NOT_FOUND,
    )
    .await;
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn rejects_closed_shape_domain_and_collection_violations_without_writes(pool: PgPool) {
    let app = setup(&pool).await;
    let mut cases = Vec::new();
    for (field, value) in [
        ("tenant_id", json!("tenant-b")),
        ("actor_id", json!("forged")),
        ("recorded_at_ms", json!(0)),
        ("id", json!(" bad")),
        ("name", json!("x".repeat(201))),
        (
            "external_identifiers",
            json!(vec![asset()["external_identifiers"][0].clone(); 101]),
        ),
    ] {
        let mut bad = asset();
        bad[field] = value;
        cases.push((ROOT.to_owned(), bad));
    }
    let mut duplicate = asset();
    duplicate["external_identifiers"] = json!([
        duplicate["external_identifiers"][0],
        duplicate["external_identifiers"][0]
    ]);
    cases.push((ROOT.to_owned(), duplicate));
    let mut nested = asset();
    nested["asset_type"]["tenant_id"] = json!("tenant-b");
    cases.push((ROOT.to_owned(), nested));
    for value in [json!(0), json!(4_294_967_296_u64)] {
        let mut bad = asset();
        bad["asset_type"]["version"] = value;
        cases.push((ROOT.to_owned(), bad));
    }
    let path = format!("{ROOT}/asset-a/device");
    for bad in [
        json!({"endpoints":[{"id":"one","kind":"bus.can","name":"CAN","extra":true}]}),
        json!({"endpoints":[{"id":"one","kind":"bus.can","name":"CAN"},{"id":"one","kind":"bus.can","name":"Other"}]}),
        json!({"endpoints":vec![device()["endpoints"][0].clone();101]}),
        json!({"asset_id":"forged","endpoints":[]}),
    ] {
        cases.push((path.clone(), bad));
    }
    for (path, bad) in cases {
        send(&app, "POST", &path, &bad, StatusCode::BAD_REQUEST).await;
    }
    for raw in [
        r#"{"id":"a","id":"b","name":"Machine","asset_type":{"id":"equipment","version":1},"external_identifiers":[]}"#.to_owned(),
        format!(r#"{{"id":"a","name":"{}","asset_type":{{"id":"equipment","version":1}},"external_identifiers":[]}}"#,"x".repeat(32_768)),
        r#"{"id":"a","name":"Machine","asset_type":{"id":"equipment","version":1,"version":2},"external_identifiers":[]}"#.to_owned(),
    ] {
        let mut req=request("POST",ROOT,&Value::Null);*req.body_mut()=Body::from(raw);
        let response=app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(),StatusCode::BAD_REQUEST);body(response).await;
    }
    assert_eq!(audit_count(&pool).await, 0);
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn accepts_collection_maxima_and_enforces_body_limit_on_otherwise_valid_json(pool: PgPool) {
    let app = setup(&pool).await;
    let mut payload = asset();
    payload["external_identifiers"] = json!(
        (0..100)
            .map(|n| json!({"kind":"serial","value":format!("serial-{n}")}))
            .collect::<Vec<_>>()
    );
    let mut exact = serde_json::to_string(&payload).unwrap();
    exact.extend(std::iter::repeat_n(' ', 32 * 1024 - exact.len()));
    let mut oversized = request("POST", ROOT, &Value::Null);
    *oversized.body_mut() = Body::from(format!("{exact} "));
    let response = app.clone().oneshot(oversized).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    body(response).await;
    assert_eq!(audit_count(&pool).await, 0);
    let mut maximum = request("POST", ROOT, &Value::Null);
    *maximum.body_mut() = Body::from(exact);
    let response = app.clone().oneshot(maximum).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    body(response).await;
    // Omitted issuer and explicit null denote the same external namespace.
    for identifier in payload["external_identifiers"].as_array_mut().unwrap() {
        identifier["authority"] = Value::Null;
    }
    send(&app, "POST", ROOT, &payload, StatusCode::OK).await;
    let endpoints: Vec<_> = (0..100)
        .map(|n| json!({"id":format!("endpoint-{n}"),"kind":"bus.can","name":format!("CAN {n}")}))
        .collect();
    let path = format!("{ROOT}/asset-a/device");
    send(
        &app,
        "POST",
        &path,
        &json!({"endpoints":endpoints}),
        StatusCode::CREATED,
    )
    .await;
    let read = send(&app, "GET", &path, &Value::Null, StatusCode::OK).await;
    assert_eq!(read["endpoints"].as_array().unwrap().len(), 100);
    assert_eq!(read["endpoints"][99]["id"], "endpoint-99");
    assert_eq!(audit_count(&pool).await, 2);
}

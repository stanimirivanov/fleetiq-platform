//! Complete content, concurrency, reference validation and immutable version behavior.
use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_publications_resolve_full_content_for_all_namespaces(pool: PgPool) {
    let app = setup(&pool).await;
    for (resource, payload) in definitions() {
        let path = format!("{ROOT}/{resource}");
        let (first, second) = tokio::join!(
            app.clone().oneshot(request("POST", &path, &payload)),
            app.clone().oneshot(request("POST", &path, &payload))
        );
        let mut statuses = Vec::new();
        for response in [first.unwrap(), second.unwrap()] {
            let status = response.status();
            statuses.push(status.as_u16());
            let expected = if status == StatusCode::CREATED {
                "published"
            } else {
                "already_published"
            };
            assert_eq!(body(response).await, json!({"outcome": expected}));
        }
        statuses.sort_unstable();
        assert_eq!(statuses, [200, 201]);
        assert_eq!(
            send(&app, "POST", &path, &payload, StatusCode::OK).await,
            json!({"outcome":"already_published"})
        );
        let mut conflicting = payload.clone();
        conflicting["name"] = json!("Different");
        let problem = send(&app, "POST", &path, &conflicting, StatusCode::CONFLICT).await;
        assert_eq!(
            problem["title"],
            "Definition version contains different immutable content"
        );
        let exact = format!("{path}/{}/versions/1", payload["id"].as_str().unwrap());
        assert_eq!(
            send(&app, "GET", &exact, &Value::Null, StatusCode::OK).await,
            payload
        );
        let mut next = payload.clone();
        next["version"] = json!(2);
        next["name"] = json!("New meaning");
        publish(&app, resource, &next).await;
        let mut competing = payload.clone();
        competing["version"] = json!(3);
        let mut different = competing.clone();
        different["name"] = json!("Competing meaning");
        let (left, right) = tokio::join!(
            app.clone().oneshot(request("POST", &path, &competing)),
            app.clone().oneshot(request("POST", &path, &different))
        );
        let mut winner = None;
        let mut statuses = Vec::new();
        for (response, content) in [(left.unwrap(), competing), (right.unwrap(), different)] {
            statuses.push(response.status().as_u16());
            if response.status() == StatusCode::CREATED {
                winner = Some(content);
            }
            body(response).await;
        }
        statuses.sort_unstable();
        assert_eq!(statuses, [201, 409]);
        let third = format!("{path}/{}/versions/3", payload["id"].as_str().unwrap());
        assert_eq!(
            send(&app, "GET", &third, &Value::Null, StatusCode::OK).await,
            winner.unwrap()
        );
        assert_eq!(
            send(&app, "GET", &exact, &Value::Null, StatusCode::OK).await,
            payload
        );
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn ordered_properties_and_every_property_field_are_immutable_content(pool: PgPool) {
    let app = setup(&pool).await;
    let property = definitions()[1].1.clone();
    publish(&app, "properties", &property).await;
    for (field, value) in [
        ("value_kind", json!("integer")),
        ("canonical_unit", json!("kelvin")),
        ("canonical_unit", Value::Null),
    ] {
        let mut changed = property.clone();
        changed[field] = value;
        send(
            &app,
            "POST",
            &format!("{ROOT}/properties"),
            &changed,
            StatusCode::CONFLICT,
        )
        .await;
    }
    let mut next_property = property.clone();
    next_property["version"] = json!(2);
    publish(&app, "properties", &next_property).await;
    let another = json!({"id":"enabled","version":1,"name":"Enabled","value_kind":"boolean"});
    publish(&app, "properties", &another).await;
    let mut asset = json!({"id":"equipment","version":1,"name":"Equipment","supported_properties":[{"id":"temperature","version":1},{"id":"enabled","version":1}]});
    publish(&app, "asset-types", &asset).await;
    send(
        &app,
        "POST",
        &format!("{ROOT}/asset-types"),
        &asset,
        StatusCode::OK,
    )
    .await;
    let mut changed_reference = asset.clone();
    changed_reference["supported_properties"][0]["version"] = json!(2);
    send(
        &app,
        "POST",
        &format!("{ROOT}/asset-types"),
        &changed_reference,
        StatusCode::CONFLICT,
    )
    .await;
    asset["supported_properties"]
        .as_array_mut()
        .unwrap()
        .reverse();
    send(
        &app,
        "POST",
        &format!("{ROOT}/asset-types"),
        &asset,
        StatusCode::CONFLICT,
    )
    .await;
    asset["supported_properties"] = json!([{"id":"enabled","version":1}]);
    send(
        &app,
        "POST",
        &format!("{ROOT}/asset-types"),
        &asset,
        StatusCode::CONFLICT,
    )
    .await;
    let exact = send(
        &app,
        "GET",
        &format!("{ROOT}/asset-types/equipment/versions/1"),
        &Value::Null,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        exact["supported_properties"],
        json!([{"id":"temperature","version":1},{"id":"enabled","version":1}])
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn missing_foreign_tenant_references_roll_back_and_legacy_writes_stay_strict(pool: PgPool) {
    let app = setup(&pool).await;
    sqlx::query("INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind) VALUES ('tenant-b','foreign',1,'Foreign','decimal')").execute(&pool).await.unwrap();
    let asset = json!({"id":"equipment","version":1,"name":"Equipment","supported_properties":[{"id":"foreign","version":1}]});
    send(
        &app,
        "POST",
        &format!("{ROOT}/asset-types"),
        &asset,
        StatusCode::NOT_FOUND,
    )
    .await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM metadata.asset_type_definitions")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    let property = definitions()[1].1.clone();
    publish(&app, "properties", &property).await;
    let loaded = fleetiq_metadata::DefinitionRepository::find_property(
        &PostgresDefinitionRepository::new(pool.clone()),
        &fleetiq_domain::TenantId::new("tenant-a").unwrap(),
        &fleetiq_domain::PropertyDefinitionRef::new(
            fleetiq_domain::PropertyDefinitionId::new("temperature").unwrap(),
            fleetiq_domain::DefinitionVersion::new(1).unwrap(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(
        fleetiq_metadata::DefinitionRepository::insert_property(
            &PostgresDefinitionRepository::new(pool.clone()),
            &fleetiq_domain::TenantId::new("tenant-a").unwrap(),
            &loaded
        )
        .await,
        Err(fleetiq_postgres::PostgresRepositoryError::Conflict { .. })
    ));
    for statement in [
        "UPDATE metadata.property_definitions SET display_name='Changed'",
        "DELETE FROM metadata.property_definitions",
    ] {
        assert!(sqlx::query(statement).execute(&pool).await.is_err());
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn malformed_and_forged_publications_never_create_versions(pool: PgPool) {
    let app = setup(&pool).await;
    for (resource, payload) in definitions() {
        for (field, value) in [
            ("tenant_id", json!("tenant-b")),
            ("administrator_actor_id", json!("forged")),
            ("recorded_at_ms", json!(1)),
            ("unknown", json!(true)),
            ("id", json!(" bad")),
            ("id", json!("x".repeat(129))),
            ("version", json!(0)),
            ("version", json!(4_294_967_296_u64)),
            ("name", json!(" ")),
            ("name", json!("x".repeat(201))),
            ("name", json!("x".repeat(20_000))),
        ] {
            let mut bad = payload.clone();
            bad[field] = value;
            send(
                &app,
                "POST",
                &format!("{ROOT}/{resource}"),
                &bad,
                StatusCode::BAD_REQUEST,
            )
            .await;
        }
        let mut duplicate = request("POST", &format!("{ROOT}/{resource}"), &Value::Null);
        *duplicate.body_mut() = Body::from(payload.to_string().replacen(
            "\"version\":1",
            "\"version\":1,\"version\":1",
            1,
        ));
        assert_eq!(
            app.clone().oneshot(duplicate).await.unwrap().status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            send(
                &app,
                "GET",
                &format!("{ROOT}/{resource}"),
                &Value::Null,
                StatusCode::OK
            )
            .await["definitions"],
            json!([])
        );
    }
    for bad in [
        json!({"id":"enabled","version":1,"name":"Enabled","value_kind":"boolean","canonical_unit":"volt"}),
        json!({"id":"value","version":1,"name":"Value","value_kind":"unknown"}),
    ] {
        send(
            &app,
            "POST",
            &format!("{ROOT}/properties"),
            &bad,
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
    let bad = json!({"id":"equipment","version":1,"name":"Equipment","supported_properties":[{"id":"same","version":1},{"id":"same","version":2}]});
    send(
        &app,
        "POST",
        &format!("{ROOT}/asset-types"),
        &bad,
        StatusCode::BAD_REQUEST,
    )
    .await;
    let too_many = json!({
        "id": "equipment", "version": 1, "name": "Equipment",
        "supported_properties": (0..101).map(|index| {
            json!({"id": format!("property-{index}"), "version": 1})
        }).collect::<Vec<_>>()
    });
    send(
        &app,
        "POST",
        &format!("{ROOT}/asset-types"),
        &too_many,
        StatusCode::BAD_REQUEST,
    )
    .await;
    send(
        &app,
        "POST",
        &format!("{ROOT}/relationship-types"),
        &definitions()[1].1,
        StatusCode::BAD_REQUEST,
    )
    .await;
}

use super::*;

#[tokio::test]
async fn authorized_property_query_returns_exact_meaning_and_unit() {
    let calls = Arc::new(AtomicUsize::new(0));
    let response = property_router(
        true,
        Ok(Some(sample_property(
            PropertyValueKind::Decimal,
            Some("Cel"),
        ))),
        calls.clone(),
    )
    .oneshot(request(PROPERTY_PATH, Some(TOKEN)))
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        to_bytes(response.into_body(), 2048).await.unwrap(),
        EXPECTED_PROPERTY_JSON
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn property_value_kinds_and_absent_unit_have_stable_wire_names() {
    for (kind, wire_name) in [
        (PropertyValueKind::Boolean, "boolean"),
        (PropertyValueKind::Integer, "integer"),
        (PropertyValueKind::Decimal, "decimal"),
        (PropertyValueKind::Text, "text"),
    ] {
        let response = property_router(
            true,
            Ok(Some(sample_property(kind, None))),
            Arc::new(AtomicUsize::new(0)),
        )
        .oneshot(request(PROPERTY_PATH, Some(TOKEN)))
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = String::from_utf8(to_bytes(response.into_body(), 2048).await.unwrap().to_vec())
            .unwrap();
        assert!(body.contains(&format!("\"value_kind\":\"{wire_name}\"")));
        assert!(body.contains("\"canonical_unit\":null"));
    }
}

#[tokio::test]
async fn property_query_requires_credentials_and_current_tenant_grant() {
    let calls = Arc::new(AtomicUsize::new(0));
    for (allowed, token, path, expected) in [
        (true, None, PROPERTY_PATH, StatusCode::UNAUTHORIZED),
        (
            true,
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            PROPERTY_PATH,
            StatusCode::UNAUTHORIZED,
        ),
        (false, Some(TOKEN), PROPERTY_PATH, StatusCode::FORBIDDEN),
        (
            true,
            Some(TOKEN),
            "/api/v1/tenants/tenant-b/properties/thermal.temperature/versions/1",
            StatusCode::FORBIDDEN,
        ),
    ] {
        let response = property_router(
            allowed,
            Ok(Some(sample_property(
                PropertyValueKind::Decimal,
                Some("Cel"),
            ))),
            calls.clone(),
        )
        .oneshot(request(path, token))
        .await
        .unwrap();
        assert_eq!(response.status(), expected);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn property_query_rejects_invalid_paths_and_does_not_choose_another_version() {
    let calls = Arc::new(AtomicUsize::new(0));
    let router = property_router(true, Ok(None), calls.clone());
    for path in [
        "/api/v1/tenants/tenant-a/properties/thermal.temperature/versions/0",
        "/api/v1/tenants/tenant-a/properties/thermal.temperature/versions/nope",
        "/api/v1/tenants/tenant-a/properties/thermal.temperature/versions/4294967296",
        "/api/v1/tenants/tenant-a/properties/thermal.temperature/versions/+1",
        "/api/v1/tenants/tenant-a/properties/%20bad/versions/1",
    ] {
        let response = router
            .clone()
            .oneshot(request(path, Some(TOKEN)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        router
            .oneshot(request(PROPERTY_PATH, Some(TOKEN)))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn property_repository_failures_are_redacted_and_other_methods_are_rejected() {
    let response = property_router(true, Err(TestFailure), Arc::new(AtomicUsize::new(0)))
        .oneshot(request(PROPERTY_PATH, Some(TOKEN)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        !String::from_utf8_lossy(&to_bytes(response.into_body(), 1024).await.unwrap())
            .contains("private definition adapter detail")
    );

    let response = property_router(
        true,
        Ok(Some(sample_property(
            PropertyValueKind::Decimal,
            Some("Cel"),
        ))),
        Arc::new(AtomicUsize::new(0)),
    )
    .oneshot(Request::post(PROPERTY_PATH).body(Body::empty()).unwrap())
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn configured_property_query_reads_pinned_version_and_current_postgres_grant(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a'), ('tenant-b')")
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ('actor-a')")
        .execute(&pool)
        .await?;
    sqlx::query(
        "INSERT INTO identity.external_actor_identities \
         (authority_id, subject, actor_id) VALUES ('workload', 'reader', 'actor-a')",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "INSERT INTO identity.tenant_action_grants \
         (tenant_id, actor_id, action) VALUES ('tenant-a', 'actor-a', 'metadata.read')",
    )
    .execute(&pool)
    .await?;

    let repository = PostgresDefinitionRepository::new(pool.clone());
    let tenant = TenantId::new("tenant-a")?;
    repository
        .insert_property(
            &tenant,
            &sample_property(PropertyValueKind::Decimal, Some("Cel")),
        )
        .await?;
    repository
        .insert_property(
            &tenant,
            &PropertyDefinition::new(
                PropertyDefinitionRef::new(
                    PropertyDefinitionId::new("thermal.temperature")?,
                    DefinitionVersion::new(2)?,
                ),
                DefinitionName::new("Revised temperature")?,
                PropertyValueKind::Integer,
                None,
            )?,
        )
        .await?;

    let bearer = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")?
        .with_bearer_identity(TOKEN, "workload", "reader")?
        .bearer_identity()
        .expect("configured credential")
        .clone();
    let router = postgres_definition_router(
        pool.clone(),
        fleetiq_access::CallerAuthenticationService::new(
            crate::bearer_authentication::ConfiguredBearerAuthenticator::new(bearer),
            fleetiq_postgres::PostgresActorIdentityResolver::new(pool.clone()),
        ),
    );
    let response = router
        .clone()
        .oneshot(request(PROPERTY_PATH, Some(TOKEN)))
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 2048).await?,
        EXPECTED_PROPERTY_JSON
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/api/v1/tenants/tenant-a/properties/thermal.temperature/versions/3",
                Some(TOKEN),
            ))
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/api/v1/tenants/tenant-b/properties/thermal.temperature/versions/1",
                Some(TOKEN),
            ))
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );

    sqlx::query(
        "DELETE FROM identity.tenant_action_grants \
         WHERE tenant_id = 'tenant-a' AND actor_id = 'actor-a' AND action = 'metadata.read'",
    )
    .execute(&pool)
    .await?;
    assert_eq!(
        router
            .oneshot(request(PROPERTY_PATH, Some(TOKEN)))
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    Ok(())
}

use super::*;

#[tokio::test]
async fn authorized_query_returns_pinned_properties_in_declaration_order() {
    let calls = Arc::new(AtomicUsize::new(0));
    let response = router(true, Ok(Some(sample_definition())), calls.clone())
        .oneshot(request(PATH, Some(TOKEN)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        to_bytes(response.into_body(), 2048).await.unwrap(),
        EXPECTED_JSON
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn credential_and_tenant_policy_failures_prevent_definition_lookup() {
    let calls = Arc::new(AtomicUsize::new(0));
    for (allowed, token, path, expected) in [
        (true, None, PATH, StatusCode::UNAUTHORIZED),
        (
            true,
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            PATH,
            StatusCode::UNAUTHORIZED,
        ),
        (false, Some(TOKEN), PATH, StatusCode::FORBIDDEN),
        (
            true,
            Some(TOKEN),
            "/api/v1/tenants/tenant-b/asset-types/generic.power-system/versions/2",
            StatusCode::FORBIDDEN,
        ),
    ] {
        let response = router(allowed, Ok(Some(sample_definition())), calls.clone())
            .oneshot(request(path, token))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn invalid_version_and_missing_definition_do_not_fall_back_to_latest() {
    let calls = Arc::new(AtomicUsize::new(0));
    let router = router(true, Ok(None), calls.clone());
    for version in ["0", "not-a-version", "4294967296", "+2"] {
        let path =
            format!("/api/v1/tenants/tenant-a/asset-types/generic.power-system/versions/{version}");
        let response = router
            .clone()
            .oneshot(request(&path, Some(TOKEN)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        router
            .oneshot(request(PATH, Some(TOKEN)))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn definition_failure_is_redacted_and_other_methods_are_rejected() {
    let failed = router(true, Err(TestFailure), Arc::new(AtomicUsize::new(0)));
    let response = failed.oneshot(request(PATH, Some(TOKEN))).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        !String::from_utf8_lossy(&to_bytes(response.into_body(), 1024).await.unwrap())
            .contains("private definition adapter detail")
    );

    let router = router(
        true,
        Ok(Some(sample_definition())),
        Arc::new(AtomicUsize::new(0)),
    );
    let response = router
        .oneshot(Request::post(PATH).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn configured_asset_type_query_uses_exact_version_and_current_postgres_grant(
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
    let definition = sample_definition();
    for reference in definition.supported_properties() {
        let property = PropertyDefinition::new(
            reference.clone(),
            DefinitionName::new(reference.id().as_str())?,
            PropertyValueKind::Decimal,
            None,
        )?;
        repository.insert_property(&tenant, &property).await?;
    }
    repository.insert_asset_type(&tenant, &definition).await?;

    let bearer = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")?
        .with_bearer_identity(TOKEN, "workload", "reader")?
        .bearer_identity()
        .expect("configured credential")
        .clone();
    let router = postgres_definition_router(pool.clone(), bearer);
    let response = router.clone().oneshot(request(PATH, Some(TOKEN))).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 2048).await?, EXPECTED_JSON);
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/api/v1/tenants/tenant-a/asset-types/generic.power-system/versions/1",
                Some(TOKEN)
            ))
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/api/v1/tenants/tenant-b/asset-types/generic.power-system/versions/2",
                Some(TOKEN)
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
        router.oneshot(request(PATH, Some(TOKEN))).await?.status(),
        StatusCode::FORBIDDEN
    );
    Ok(())
}

use super::super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn every_creation_field_and_endpoint_order_participates_in_retry_equality(
    pool: PgPool,
) -> Result {
    seed(&pool).await?;
    let mut original = asset("tenant-a", "asset-a");
    original.add_external_identifier(identifier("a", None, "1"))?;
    original.add_external_identifier(identifier("b", Some("maker"), "2"))?;
    register(&pool, &MetadataRegistration::Asset(original.clone())).await?;
    let mut reordered = asset("tenant-a", "asset-a");
    for id in original.external_identifiers().iter().rev() {
        reordered.add_external_identifier(id.clone())?;
    }
    assert!(matches!(
        register(&pool, &MetadataRegistration::Asset(reordered)).await?,
        RegistrationOutcome::AlreadyRegistered(_)
    ));
    let mut renamed = original.clone();
    renamed.rename(AssetName::new("Changed")?);
    assert_eq!(
        register(&pool, &MetadataRegistration::Asset(renamed)).await?,
        RegistrationOutcome::Conflict
    );
    assert_eq!(
        register(
            &pool,
            &MetadataRegistration::Asset(asset("tenant-a", "asset-a"))
        )
        .await?,
        RegistrationOutcome::Conflict
    );
    sqlx::query("INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count) VALUES ('tenant-a','equipment',2,'V2',0)").execute(&pool).await?;
    let changed_type = Asset::new(
        TenantId::new("tenant-a")?,
        AssetId::new("asset-a")?,
        AssetTypeRef::new(AssetTypeId::new("equipment")?, DefinitionVersion::new(2)?),
        AssetName::new("Machine")?,
    );
    assert_eq!(
        register(&pool, &MetadataRegistration::Asset(changed_type)).await?,
        RegistrationOutcome::Conflict
    );
    register(
        &pool,
        &MetadataRegistration::Device(device("tenant-a", "asset-a", &["a", "b"])),
    )
    .await?;
    for endpoints in [&["b", "a"][..], &["a"][..], &["a", "c"][..]] {
        assert_eq!(
            register(
                &pool,
                &MetadataRegistration::Device(device("tenant-a", "asset-a", endpoints))
            )
            .await?,
            RegistrationOutcome::Conflict
        );
    }
    assert_eq!(counts(&pool).await?, (1, 2, 1, 2, 2));
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn tenant_and_authority_namespaces_remain_independent(pool: PgPool) -> Result {
    seed(&pool).await?;
    for (tenant, id, authority) in [
        ("tenant-a", "shared", None),
        ("tenant-b", "shared", None),
        ("tenant-a", "other", Some("maker")),
    ] {
        let mut item = asset(tenant, id);
        item.add_external_identifier(identifier("serial", authority, "same"))?;
        register(&pool, &MetadataRegistration::Asset(item)).await?;
    }
    assert_eq!(counts(&pool).await?, (3, 3, 0, 0, 3));
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn invalid_references_and_failed_audit_leave_no_parent_or_children(pool: PgPool) -> Result {
    // A definition exists only in the other tenant; published definitions cannot
    // be deleted just to manufacture absence in a test fixture.
    sqlx::raw_sql("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a'),('tenant-b');
        INSERT INTO identity.actors (actor_id) VALUES ('operator-a'),('operator-b');
        INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count)
        VALUES ('tenant-b','equipment',1,'Equipment',0);").execute(&pool).await?;
    assert!(matches!(
        PostgresRegistrationRepository::new(pool.clone())
            .register(
                &MetadataRegistration::Asset(asset("tenant-a", "missing-type")),
                &actor(),
                UnixTimeMillis::new(0)
            )
            .await,
        Err(PostgresRepositoryError::InvalidReference { .. })
    ));
    // The same ID exists only in the other tenant: capability must not attach there.
    PostgresAssetRepository::new(pool.clone())
        .insert(&asset("tenant-b", "foreign"))
        .await?;
    assert!(matches!(
        PostgresRegistrationRepository::new(pool.clone())
            .register(
                &MetadataRegistration::Device(device("tenant-a", "foreign", &["a"])),
                &actor(),
                UnixTimeMillis::new(0)
            )
            .await,
        Err(PostgresRepositoryError::InvalidReference { .. })
    ));
    let mut value = asset("tenant-b", "bad-audit");
    value.add_external_identifier(identifier("serial", None, "bad-audit"))?;
    assert!(matches!(
        PostgresRegistrationRepository::new(pool.clone())
            .register(
                &MetadataRegistration::Asset(value),
                &ActorId::new("missing-actor")?,
                UnixTimeMillis::new(0)
            )
            .await,
        Err(PostgresRepositoryError::InvalidReference { .. })
    ));
    assert!(matches!(
        PostgresRegistrationRepository::new(pool.clone())
            .register(
                &MetadataRegistration::Device(device("tenant-b", "foreign", &["a"])),
                &ActorId::new("missing-actor")?,
                UnixTimeMillis::new(0)
            )
            .await,
        Err(PostgresRepositoryError::InvalidReference { .. })
    ));
    assert_eq!(counts(&pool).await?, (1, 0, 0, 0, 0));
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn legacy_strict_inserts_do_not_acquire_fabricated_attribution(pool: PgPool) -> Result {
    seed(&pool).await?;
    let value = asset("tenant-a", "legacy");
    PostgresAssetRepository::new(pool.clone())
        .insert(&value)
        .await?;
    assert_eq!(
        register(&pool, &MetadataRegistration::Asset(value)).await?,
        RegistrationOutcome::Conflict
    );
    let value = device("tenant-a", "legacy", &[]);
    PostgresDeviceRepository::new(pool.clone())
        .insert(&value)
        .await?;
    assert_eq!(
        register(&pool, &MetadataRegistration::Device(value)).await?,
        RegistrationOutcome::Conflict
    );
    assert_eq!(counts(&pool).await?, (1, 0, 1, 0, 0));
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn evidence_is_immutable_and_device_evidence_requires_capability(pool: PgPool) -> Result {
    seed(&pool).await?;
    register(
        &pool,
        &MetadataRegistration::Asset(asset("tenant-a", "asset-a")),
    )
    .await?;
    for statement in [
        "UPDATE metadata.registration_records SET recorded_at_ms=999",
        "DELETE FROM metadata.registration_records",
        "TRUNCATE metadata.registration_records",
    ] {
        let error = sqlx::query(statement).execute(&pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    let error = sqlx::query("INSERT INTO metadata.registration_records (tenant_id,asset_id,registration_kind,actor_id,recorded_at_ms) VALUES ('tenant-a','asset-a','device','operator-a',123)").execute(&pool).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    assert_eq!(
        register(
            &pool,
            &MetadataRegistration::Asset(asset("tenant-a", "asset-a"))
        )
        .await?,
        RegistrationOutcome::AlreadyRegistered(receipt())
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn deferred_commit_failure_is_an_error_and_exact_retry_can_recover(pool: PgPool) -> Result {
    seed(&pool).await?;
    sqlx::raw_sql("CREATE FUNCTION metadata.fail_registration_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
        BEGIN RAISE EXCEPTION 'synthetic deferred failure' USING ERRCODE='23514'; END; $$;
        CREATE CONSTRAINT TRIGGER tr_registration_commit_fixture AFTER INSERT ON metadata.registration_records
        DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION metadata.fail_registration_commit();")
        .execute(&pool).await?;
    let value = MetadataRegistration::Asset(asset("tenant-a", "asset-a"));
    let error = PostgresRegistrationRepository::new(pool.clone())
        .register(&value, &actor(), UnixTimeMillis::new(123))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        PostgresRepositoryError::InvariantViolation { .. }
    ));
    assert_eq!(counts(&pool).await?, (0, 0, 0, 0, 0));
    sqlx::query("DROP TRIGGER tr_registration_commit_fixture ON metadata.registration_records")
        .execute(&pool)
        .await?;
    assert_eq!(
        register(&pool, &value).await?,
        RegistrationOutcome::Registered(receipt())
    );
    // Discard the first response as a caller would after a lost connection.
    assert_eq!(
        register(&pool, &value).await?,
        RegistrationOutcome::AlreadyRegistered(receipt())
    );
    Ok(())
}

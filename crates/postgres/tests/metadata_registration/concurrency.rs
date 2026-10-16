use super::super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_equal_requests_have_one_winner_for_both_creation_kinds(pool: PgPool) -> Result {
    seed(&pool).await?;
    let mut asset = asset("tenant-a", "asset-a");
    asset.add_external_identifier(identifier("imei", None, "350000000000001"))?;
    for request in [
        MetadataRegistration::Asset(asset),
        MetadataRegistration::Device(device("tenant-a", "asset-a", &["a", "b"])),
    ] {
        let (left, right) = tokio::join!(register(&pool, &request), register(&pool, &request));
        let results = [left?, right?];
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, RegistrationOutcome::Registered(_)))
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, RegistrationOutcome::AlreadyRegistered(_)))
                .count(),
            1
        );
    }
    assert_eq!(counts(&pool).await?, (1, 1, 1, 2, 2));
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_conflicting_content_preserves_the_committed_winner(pool: PgPool) -> Result {
    seed(&pool).await?;
    let left = asset("tenant-a", "asset-a");
    let mut right = left.clone();
    right.rename(AssetName::new("Different")?);
    let left = MetadataRegistration::Asset(left);
    let right = MetadataRegistration::Asset(right);
    let (a, b) = tokio::join!(register(&pool, &left), register(&pool, &right));
    let outcomes = [a?, b?];
    assert_eq!(
        outcomes
            .iter()
            .filter(|r| matches!(r, RegistrationOutcome::Registered(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|r| matches!(r, RegistrationOutcome::Conflict))
            .count(),
        1
    );
    assert_eq!(counts(&pool).await?, (1, 0, 0, 0, 1));
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_external_identity_claims_cannot_create_partial_losers(pool: PgPool) -> Result {
    seed(&pool).await?;
    let mut left = asset("tenant-a", "left");
    let mut right = asset("tenant-a", "right");
    // The same unordered identifier set arrives in opposite declaration orders.
    // Both inserts must acquire external-identity uniqueness locks consistently.
    let claims = [
        identifier("imei", None, "350000000000001"),
        identifier("serial", Some("maker"), "SN-1"),
    ];
    for claim in &claims {
        left.add_external_identifier(claim.clone())?;
    }
    for claim in claims.into_iter().rev() {
        right.add_external_identifier(claim)?;
    }
    let left = MetadataRegistration::Asset(left);
    let right = MetadataRegistration::Asset(right);
    let (a, b) = tokio::join!(register(&pool, &left), register(&pool, &right));
    let results = [a?, b?];
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, RegistrationOutcome::Registered(_)))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, RegistrationOutcome::Conflict))
            .count(),
        1
    );
    assert_eq!(counts(&pool).await?, (1, 2, 0, 0, 1));
    Ok(())
}

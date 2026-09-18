//! Integration tests for the PostgreSQL protocol-package catalogue adapter.

use std::{error::Error, num::NonZeroU64};

use fleetiq_application::{
    ProtocolPackageCatalog, ProtocolPackageRegistrationOutcome, ProtocolPackageRelease,
};
use fleetiq_domain::TenantId;
use fleetiq_postgres::{
    PostgresProtocolPackageCatalog, PostgresProtocolPackageCatalogError, PostgresRepositoryError,
};
use fleetiq_telemetry::{
    ProtocolArtifactDigest, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};
use sqlx::PgPool;

async fn seed_tenant(pool: &PgPool, tenant_id: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ($1)")
        .bind(tenant_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn first_registration_and_lookup_preserve_exact_metadata(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_tenant(&pool, "tenant-a").await?;
    let catalog = PostgresProtocolPackageCatalog::new(pool);
    let release = release("tenant-a", [1; 32], u64::MAX);

    let outcome = catalog.register(&release).await?;
    let found = catalog.find(release.tenant_id(), release.package()).await?;

    assert_eq!(outcome, ProtocolPackageRegistrationOutcome::Registered);
    assert_eq!(found.as_ref(), Some(&release));
    assert_eq!(
        found
            .expect("registered release must exist")
            .artifact_size_bytes()
            .get(),
        u64::MAX
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn exact_retry_returns_already_present(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_tenant(&pool, "tenant-a").await?;
    let catalog = PostgresProtocolPackageCatalog::new(pool);
    let release = release("tenant-a", [1; 32], 500);

    assert_eq!(
        catalog.register(&release).await?,
        ProtocolPackageRegistrationOutcome::Registered
    );
    assert_eq!(
        catalog.register(&release).await?,
        ProtocolPackageRegistrationOutcome::AlreadyPresent
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_exact_registrations_have_one_winner(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_tenant(&pool, "tenant-a").await?;
    let catalog = PostgresProtocolPackageCatalog::new(pool);
    let release = release("tenant-a", [1; 32], 500);

    let (first, second) = tokio::join!(catalog.register(&release), catalog.register(&release));
    let outcomes = [first?, second?];

    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == ProtocolPackageRegistrationOutcome::Registered)
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == ProtocolPackageRegistrationOutcome::AlreadyPresent)
            .count(),
        1
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn reused_release_key_with_different_content_is_a_collision(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_tenant(&pool, "tenant-a").await?;
    let catalog = PostgresProtocolPackageCatalog::new(pool);
    let original = release("tenant-a", [1; 32], 500);
    catalog.register(&original).await?;

    for conflicting in [
        release("tenant-a", [2; 32], 500),
        release("tenant-a", [1; 32], 501),
    ] {
        let error = catalog
            .register(&conflicting)
            .await
            .expect_err("changed immutable metadata must be a collision");
        assert!(matches!(
            error,
            PostgresProtocolPackageCatalogError::IdentityCollision {
                ref tenant_id,
                ref package,
            } if tenant_id.as_str() == "tenant-a"
                && package.id().as_str() == "custom.binary"
                && package.version().as_str() == "2026.09.1"
        ));
    }
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn lookup_is_tenant_scoped_and_absence_is_explicit(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_tenant(&pool, "tenant-a").await?;
    seed_tenant(&pool, "tenant-b").await?;
    let catalog = PostgresProtocolPackageCatalog::new(pool);
    let first = release("tenant-a", [1; 32], 100);
    let second = release("tenant-b", [2; 32], 200);
    catalog.register(&first).await?;
    catalog.register(&second).await?;

    assert_eq!(
        catalog.find(first.tenant_id(), first.package()).await?,
        Some(first)
    );
    assert_eq!(
        catalog.find(second.tenant_id(), second.package()).await?,
        Some(second)
    );
    assert_eq!(
        catalog
            .find(
                &TenantId::new("tenant-c")?,
                &package("custom.binary", "2026.09.1")?,
            )
            .await?,
        None
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn registration_requires_an_existing_tenant(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let catalog = PostgresProtocolPackageCatalog::new(pool);
    let release = release("missing-tenant", [1; 32], 500);

    let error = catalog
        .register(&release)
        .await
        .expect_err("release registration must preserve tenant ownership");

    assert!(matches!(
        error,
        PostgresProtocolPackageCatalogError::Repository(
            PostgresRepositoryError::InvalidReference {
                constraint: Some(constraint),
                ..
            }
        ) if constraint == "fk_protocol_package_releases_tenant"
    ));
    Ok(())
}

fn release(tenant: &str, digest: [u8; 32], size: u64) -> ProtocolPackageRelease {
    ProtocolPackageRelease::new(
        TenantId::new(tenant).expect("test tenant is valid"),
        package("custom.binary", "2026.09.1").expect("test package is valid"),
        ProtocolArtifactDigest::from_sha256(digest),
        NonZeroU64::new(size).expect("test artifact size is nonzero"),
    )
}

fn package(
    id: &str,
    version: &str,
) -> Result<ProtocolPackageRef, fleetiq_telemetry::PipelineIdentifierError> {
    Ok(ProtocolPackageRef::new(
        ProtocolPackageId::new(id)?,
        ProtocolPackageVersion::new(version)?,
    ))
}

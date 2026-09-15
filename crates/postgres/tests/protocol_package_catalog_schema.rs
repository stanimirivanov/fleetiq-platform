//! Integration tests for protocol package catalogue storage invariants.

use std::error::Error;

use sqlx::{PgPool, Row};

async fn seed_tenant(pool: &PgPool, tenant_id: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ($1)")
        .bind(tenant_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn insert_release(
    pool: &PgPool,
    tenant_id: &str,
    package_id: &str,
    version: &str,
    digest: &[u8],
    size: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        r"
        INSERT INTO metadata.protocol_package_releases
            (tenant_id, protocol_package_id, protocol_package_version,
             artifact_sha256, artifact_size_bytes)
        VALUES ($1, $2, $3, $4, $5::numeric)
        ",
    )
    .bind(tenant_id)
    .bind(package_id)
    .bind(version)
    .bind(digest)
    .bind(size)
    .execute(pool)
    .await?;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn release_preserves_exact_content_metadata(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_tenant(&pool, "tenant-a").await?;
    let digest = (0_u8..32).collect::<Vec<_>>();
    insert_release(
        &pool,
        "tenant-a",
        "custom.binary",
        "2026.09.1",
        &digest,
        "18446744073709551615",
    )
    .await?;

    let row = sqlx::query(
        r"
        SELECT tenant_id, protocol_package_id, protocol_package_version,
               artifact_sha256, artifact_size_bytes::text AS artifact_size_text
        FROM metadata.protocol_package_releases
        ",
    )
    .fetch_one(&pool)
    .await?;

    assert_eq!(row.try_get::<String, _>("tenant_id")?, "tenant-a");
    assert_eq!(
        row.try_get::<String, _>("protocol_package_id")?,
        "custom.binary"
    );
    assert_eq!(
        row.try_get::<String, _>("protocol_package_version")?,
        "2026.09.1"
    );
    assert_eq!(row.try_get::<Vec<u8>, _>("artifact_sha256")?, digest);
    assert_eq!(
        row.try_get::<String, _>("artifact_size_text")?,
        "18446744073709551615"
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn release_identity_is_tenant_scoped(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_tenant(&pool, "tenant-a").await?;
    seed_tenant(&pool, "tenant-b").await?;
    insert_release(&pool, "tenant-a", "shared", "1", &[1; 32], "10").await?;
    insert_release(&pool, "tenant-b", "shared", "1", &[2; 32], "20").await?;

    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM metadata.protocol_package_releases WHERE protocol_package_id = 'shared'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(count, 2);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn release_requires_an_existing_tenant_and_valid_identifiers(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_tenant(&pool, "tenant-a").await?;

    for (tenant, package, version) in [
        ("missing", "valid", "1"),
        ("tenant-a", " invalid", "1"),
        ("tenant-a", "valid", "bad\nversion"),
    ] {
        assert!(
            insert_release(&pool, tenant, package, version, &[1; 32], "10")
                .await
                .is_err()
        );
    }
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn digest_and_size_require_exact_supported_representations(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_tenant(&pool, "tenant-a").await?;

    for (case, digest, size) in [
        ("short-digest", vec![1; 31], "1"),
        ("long-digest", vec![1; 33], "1"),
        ("zero-size", vec![1; 32], "0"),
        ("negative-size", vec![1; 32], "-1"),
        ("fractional-size", vec![1; 32], "1.5"),
        ("oversized", vec![1; 32], "18446744073709551616"),
    ] {
        assert!(
            insert_release(&pool, "tenant-a", case, "1", &digest, size)
                .await
                .is_err(),
            "invalid case {case} must be rejected"
        );
    }
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn release_key_is_unique_and_rows_are_immutable(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_tenant(&pool, "tenant-a").await?;
    insert_release(&pool, "tenant-a", "custom", "1", &[1; 32], "10").await?;

    assert!(
        insert_release(&pool, "tenant-a", "custom", "1", &[1; 32], "10")
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE metadata.protocol_package_releases SET artifact_size_bytes = 11",)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM metadata.protocol_package_releases")
            .execute(&pool)
            .await
            .is_err()
    );

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM metadata.protocol_package_releases")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 1);
    Ok(())
}

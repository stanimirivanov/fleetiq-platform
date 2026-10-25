//! Real PostgreSQL atomicity, concurrency and immutable evidence boundaries.
use fleetiq_domain::{ActorId, TenantId, UnixTimeMillis};
use fleetiq_ingestion::{
    PROTOCOL_ADMISSION_POLICY, ProtocolAdmission, ProtocolAdmissionWorld, ProtocolArtifact,
    ProtocolArtifactStore, ProtocolPackageCatalog, ProtocolPackageRelease,
};
use fleetiq_postgres::{PostgresProtocolArtifactStore, PostgresProtocolPackageCatalog};
use fleetiq_telemetry::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};
use sqlx::PgPool;
use std::{error::Error, num::NonZeroU64};
fn release(tenant: &str, artifact: &ProtocolArtifact) -> ProtocolPackageRelease {
    ProtocolPackageRelease::new(
        TenantId::new(tenant).unwrap(),
        ProtocolPackageRef::new(
            ProtocolPackageId::new("custom.binary").unwrap(),
            ProtocolPackageVersion::new("1").unwrap(),
        ),
        artifact.digest(),
        NonZeroU64::new(artifact.bytes().len() as u64).unwrap(),
    )
}
async fn seed(pool: &PgPool) {
    sqlx::raw_sql("INSERT INTO metadata.tenants(tenant_id) VALUES ('tenant-a'),('tenant-b'); INSERT INTO identity.actors(actor_id) VALUES ('operator-a'),('operator-b');").execute(pool).await.unwrap();
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_storage_and_admission_retain_one_exact_first_receipt(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await;
    let artifact = ProtocolArtifact::new(b"opaque-component".to_vec())?;
    let r = release("tenant-a", &artifact);
    let catalog = PostgresProtocolPackageCatalog::new(pool.clone());
    catalog.register(&r).await?;
    let store = PostgresProtocolArtifactStore::new(pool.clone());
    let (a, b) = tokio::join!(store.put(&r, &artifact), store.put(&r, &artifact));
    a?;
    b?;
    assert_eq!(store.find(&r).await?, Some(artifact.clone()));
    assert!(store.find(&release("tenant-b", &artifact)).await?.is_none());
    let another = ProtocolPackageRelease::new(
        r.tenant_id().clone(),
        ProtocolPackageRef::new(r.package().id().clone(), ProtocolPackageVersion::new("2")?),
        r.artifact_digest(),
        r.artifact_size_bytes(),
    );
    catalog.register(&another).await?;
    store.put(&another, &artifact).await?;
    let tenant_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ingestion.protocol_artifacts WHERE tenant_id='tenant-a'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        tenant_count, 1,
        "equal releases share one tenant-local content blob"
    );
    let foreign = release("tenant-b", &artifact);
    catalog.register(&foreign).await?;
    store.put(&foreign, &artifact).await?;
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM ingestion.protocol_artifacts")
        .fetch_one(&pool)
        .await?;
    assert_eq!(
        total, 2,
        "content identity does not imply cross-tenant ownership"
    );
    let first = ProtocolAdmission {
        release: r.clone(),
        world: ProtocolAdmissionWorld::Runtime,
        policy_version: PROTOCOL_ADMISSION_POLICY,
        actor_id: ActorId::new("operator-a")?,
        admitted_at: UnixTimeMillis::new(100),
    };
    let mut other = first.clone();
    other.actor_id = ActorId::new("operator-b")?;
    other.admitted_at = UnixTimeMillis::new(200);
    let (a, b) = tokio::join!(store.record(&first), store.record(&other));
    let a = a?;
    assert_eq!(a, b?);
    assert!(a == first || a == other);
    assert!(
        store
            .admission(&r, ProtocolAdmissionWorld::Identity)
            .await?
            .is_none()
    );
    for sql in [
        "UPDATE ingestion.protocol_artifacts SET artifact_bytes='bad'::bytea",
        "DELETE FROM ingestion.protocol_admissions",
    ] {
        assert!(sqlx::query(sql).execute(&pool).await.is_err());
    }
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn missing_catalogue_and_wrong_content_cannot_become_stored_or_admitted(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await;
    let artifact = ProtocolArtifact::new(vec![1])?;
    let r = release("tenant-a", &artifact);
    let store = PostgresProtocolArtifactStore::new(pool.clone());
    assert!(store.put(&r, &artifact).await.is_err());
    PostgresProtocolPackageCatalog::new(pool.clone())
        .register(&r)
        .await?;
    assert!(
        store
            .put(&r, &ProtocolArtifact::new(vec![2])?)
            .await
            .is_err()
    );
    assert!(store.find(&r).await?.is_none());
    let receipt = ProtocolAdmission {
        release: r,
        world: ProtocolAdmissionWorld::Runtime,
        policy_version: PROTOCOL_ADMISSION_POLICY,
        actor_id: ActorId::new("operator-a")?,
        admitted_at: UnixTimeMillis::new(1),
    };
    assert!(store.record(&receipt).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM ingestion.protocol_admissions")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 0);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn shared_digest_never_substitutes_for_exact_catalogue_length(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await;
    let artifact = ProtocolArtifact::new(vec![1])?;
    let real = release("tenant-a", &artifact);
    let catalog = PostgresProtocolPackageCatalog::new(pool.clone());
    catalog.register(&real).await?;
    let store = PostgresProtocolArtifactStore::new(pool);
    store.put(&real, &artifact).await?;
    let key = ProtocolPackageRef::new(
        real.package().id().clone(),
        ProtocolPackageVersion::new("2")?,
    );
    let inconsistent = ProtocolPackageRelease::new(
        real.tenant_id().clone(),
        key.clone(),
        real.artifact_digest(),
        NonZeroU64::new(99).unwrap(),
    );
    catalog.register(&inconsistent).await?;
    let forged = ProtocolPackageRelease::new(
        real.tenant_id().clone(),
        key,
        real.artifact_digest(),
        real.artifact_size_bytes(),
    );
    assert!(store.find(&forged).await?.is_none());
    assert!(store.put(&forged, &artifact).await.is_err());
    assert!(store.find(&inconsistent).await.is_err());
    Ok(())
}

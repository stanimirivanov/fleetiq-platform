//! Executable expiry reconciliation against retained artifacts and live fences.
use fleetiq_domain::{
    AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, SignalTarget, TenantId,
};
use fleetiq_history::{
    archive_work::{ArchiveClaim, ArchiveObject, ArchiveWorkRepository},
    exports::{ExportClaim, ExportKey, ExportRepository, ExportRequest},
    restore::{RestoreClaim, RestoreRepository, RestoredSegment},
};
use fleetiq_postgres::{PostgresArchiveWork, PostgresExports, PostgresRestore};
use sha2::{Digest, Sha256};
use sqlx::{ConnectOptions, PgPool};
use std::error::Error;
use tokio::process::Command;

fn cleanup() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fleetiq-server"));
    command
        .arg("expire-history-once")
        .env_clear()
        .kill_on_drop(true);
    command
}

#[tokio::test]
async fn cleanup_requires_explicit_valid_tenant() {
    for value in [None, Some("")] {
        let mut command = cleanup();
        if let Some(value) = value {
            command.env("FLEETIQ_HISTORY_CLEANUP_TENANT_ID", value);
        }
        let output = command.output().await.unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stdout.len(), 0);
        assert_eq!(
            std::str::from_utf8(&output.stderr).unwrap().trim(),
            "invalid history cleanup tenant"
        );
    }
}

async fn stage_restore(pool: &PgPool, tenant: &TenantId) -> Result<i64, Box<dyn Error>> {
    let archives = PostgresArchiveWork::new(pool.clone());
    let ArchiveClaim::Claimed(selection) = archives.claim(tenant).await? else {
        panic!("expected a frozen archive selection");
    };
    let bytes = fleetiq_archive::encode(tenant, &selection.changes)?;
    let object = ArchiveObject::new(Sha256::digest(&bytes).into(), i64::try_from(bytes.len())?)?;
    assert!(archives.complete(&selection, object).await?);
    let restores = PostgresRestore::new(pool.clone());
    let ticket = restores.request(tenant, selection.job_id, 0).await?;
    let RestoreClaim::Claimed(lease) = restores.claim(tenant, ticket).await? else {
        panic!("expected a restore lease");
    };
    let segment = RestoredSegment::new(bytes, selection.changes)?;
    assert!(restores.complete(&lease, &segment).await?);
    sqlx::query("UPDATE history.restore_jobs SET expires_at=clock_timestamp() WHERE tenant_id=$1")
        .bind(tenant.as_str())
        .execute(pool)
        .await?;
    Ok(selection.job_id)
}

async fn stage_exports(pool: &PgPool, tenant: &TenantId) -> Result<(), Box<dyn Error>> {
    let request = ExportRequest::new(
        tenant.clone(),
        SignalTarget::new(
            AssetId::new("asset-a")?,
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("electrical.voltage")?,
                DefinitionVersion::new(1)?,
            ),
        ),
        0,
        3000,
        None,
    )?;
    let exports = PostgresExports::new(pool.clone());
    for index in 0..=17 {
        let key = ExportKey::new(format!("cleanup-{index}"))?;
        let ticket = fleetiq_server::request_export(pool.clone(), &key, &request).await?;
        if index <= 2 {
            assert!(matches!(
                exports.claim(tenant, ticket).await?,
                ExportClaim::Claimed(_)
            ));
        }
    }
    sqlx::query("UPDATE history.export_jobs SET lease_until=clock_timestamp() WHERE tenant_id=$1 AND request_key='cleanup-2'")
        .bind(tenant.as_str())
        .execute(pool)
        .await?;
    sqlx::query("UPDATE history.export_jobs SET expires_at=clock_timestamp() WHERE tenant_id=$1 AND request_key<>'cleanup-0'")
        .bind(tenant.as_str()).execute(pool).await?;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn cleanup_drains_quiet_tenant_in_bounded_turns(pool: PgPool) -> Result<(), Box<dyn Error>> {
    sqlx::raw_sql(include_str!(
        "../../postgres/tests/property_history/topology.sql"
    ))
    .execute(&pool)
    .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,24,134217728)").execute(&pool).await?;
    let tenant = TenantId::new("tenant-a")?;
    let manifest = stage_restore(&pool, &tenant).await?;
    stage_exports(&pool, &tenant).await?;
    let database = pool.connect_options().to_url_lossy();
    for (receipt, remaining) in [
        (
            "history cleanup restores=1 exports=16 export_leases=1",
            2_i64,
        ),
        ("history cleanup restores=0 exports=1 export_leases=0", 1),
        ("history cleanup restores=0 exports=0 export_leases=0", 1),
    ] {
        let output = cleanup()
            .env("FLEETIQ_HISTORY_CLEANUP_TENANT_ID", tenant.as_str())
            .env("DATABASE_URL", database.as_str())
            .output()
            .await?;
        assert!(output.status.success(), "cleanup must succeed");
        assert_eq!(output.stderr.len(), 0);
        assert_eq!(std::str::from_utf8(&output.stdout)?.trim(), receipt);
        let retained: i64 = sqlx::query_scalar("SELECT count(*) FROM history.export_jobs WHERE tenant_id='tenant-a' AND payload IS NOT NULL").fetch_one(&pool).await?;
        assert_eq!(retained, remaining);
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.export_jobs WHERE tenant_id='tenant-a' AND status='expired' AND payload IS NULL").fetch_one(&pool).await?, 17
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.export_jobs WHERE tenant_id='tenant-a' AND request_key IN ('cleanup-0','cleanup-1') AND attempt=1 AND lease_until IS NOT NULL").fetch_one(&pool).await?, 2
    );
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT status='expired' AND request_generation=1 AND payload IS NULL FROM history.restore_jobs WHERE tenant_id='tenant-a' AND manifest_id=$1").bind(manifest).fetch_one(&pool).await?
    );
    assert!(
        PostgresArchiveWork::new(pool)
            .manifest(&tenant, manifest)
            .await?
            .is_some()
    );
    Ok(())
}

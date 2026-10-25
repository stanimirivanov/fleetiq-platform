//! Real concurrency, persisted invariants and host-scoped routing eligibility.
#[path = "protocol_routing/support.rs"]
mod support;
use fleetiq_domain::{ActorId, AssetId, ExternalIdentifierAuthority, TenantId, UnixTimeMillis};
use fleetiq_ingestion::{
    PROTOCOL_ADMISSION_POLICY, ProtocolAdmission, ProtocolAdmissionWorld, ProtocolArtifact,
    ProtocolArtifactStore, ProtocolChannelId, ProtocolDeviceRoute, ProtocolPackageCatalog,
    ProtocolPackageRelease, ProtocolRouteCommand, ProtocolRouteSelection, ProtocolRouteWrite,
    ProtocolRoutingRepository, ProtocolRoutingService, RegisteredIdentityChannel,
};
use fleetiq_postgres::{
    PostgresProtocolArtifactStore, PostgresProtocolPackageCatalog,
    PostgresProtocolRoutingRepository,
};
use fleetiq_protocol_sdk::DeviceIdentityClaim;
use fleetiq_telemetry::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};
use sqlx::PgPool;
use support::*;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn revision_races_and_exact_retries_preserve_history(pool: PgPool) {
    let selection = seed(&pool).await;
    let repository = PostgresProtocolRoutingRepository::new(pool.clone());
    let initial = command(0, Some(selection.clone()));
    let first = committed(repository.change(&initial).await.unwrap());
    let mut retry = initial.clone();
    retry.recorded_at = UnixTimeMillis::new(200);
    assert_eq!(committed(repository.change(&retry).await.unwrap()), first);
    let mut left = command(1, None);
    left.actor_id = ActorId::new("operator-b").unwrap();
    let right = command(1, Some(selection));
    let (a, b) = tokio::join!(repository.change(&left), repository.change(&right));
    let results = [a.unwrap(), b.unwrap()];
    assert_eq!(
        results
            .iter()
            .filter(|value| matches!(value, ProtocolRouteWrite::Committed(_)))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|value| matches!(value, ProtocolRouteWrite::Conflict))
            .count(),
        1
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM ingestion.protocol_route_revisions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(
        committed(repository.change(&retry).await.unwrap()),
        first,
        "historical delivery retry must not overwrite the new head"
    );
    assert!(matches!(
        repository.change(&command(0, None)).await.unwrap(),
        ProtocolRouteWrite::Conflict
    ));
    assert!(matches!(
        repository.change(&command(u32::MAX, None)).await.unwrap(),
        ProtocolRouteWrite::Conflict
    ));
    let mut backwards = command(2, None);
    backwards.recorded_at = UnixTimeMillis::new(99);
    assert!(matches!(
        repository.change(&backwards).await.unwrap(),
        ProtocolRouteWrite::Conflict
    ));
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn database_rejects_forged_heads_future_history_and_mutation(pool: PgPool) {
    let selection = seed(&pool).await;
    committed(
        PostgresProtocolRoutingRepository::new(pool.clone())
            .change(&command(0, Some(selection)))
            .await
            .unwrap(),
    );
    for statement in [
        "UPDATE ingestion.protocol_route_heads SET revision=0",
        "UPDATE ingestion.protocol_route_heads SET revision=3",
        "UPDATE ingestion.protocol_route_revisions SET actor_id='operator-b'",
        "DELETE FROM ingestion.protocol_route_revisions",
        "INSERT INTO ingestion.protocol_route_revisions(tenant_id,device_asset_id,channel_id,revision,actor_id,recorded_at_ms) VALUES ('tenant-a','device-a','tcp-a',2,'operator-a',100)",
        "INSERT INTO ingestion.protocol_route_heads(tenant_id,device_asset_id,channel_id,revision) VALUES ('tenant-a','device-b','tcp-a',0)",
    ] {
        assert!(
            sqlx::query(statement).execute(&pool).await.is_err(),
            "{statement}"
        );
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn activation_requires_device_content_world_and_current_admission(pool: PgPool) {
    let mut selection = seed(&pool).await;
    let repository = PostgresProtocolRoutingRepository::new(pool.clone());
    for device in ["missing", "plain-asset"] {
        let mut request = command(0, Some(selection.clone()));
        request.device_asset_id = AssetId::new(device).unwrap();
        assert_eq!(
            repository.change(&request).await.unwrap(),
            ProtocolRouteWrite::Unavailable
        );
    }
    selection.world = ProtocolAdmissionWorld::Runtime;
    assert_eq!(
        repository
            .change(&command(0, Some(selection.clone())))
            .await
            .unwrap(),
        ProtocolRouteWrite::Unavailable
    );
    selection.world = ProtocolAdmissionWorld::Identity;
    selection.package = ProtocolPackageRef::new(
        selection.package.id().clone(),
        ProtocolPackageVersion::new("missing").unwrap(),
    );
    assert_eq!(
        repository
            .change(&command(0, Some(selection)))
            .await
            .unwrap(),
        ProtocolRouteWrite::Unavailable
    );
    assert!(
        repository
            .find(
                &TenantId::new("tenant-a").unwrap(),
                &AssetId::new("device-a").unwrap(),
                &ProtocolChannelId::new("tcp-a").unwrap()
            )
            .await
            .unwrap()
            .is_none()
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn lookup_never_escapes_tenant_channel_issuer_or_decoder_scope(pool: PgPool) {
    let selection = seed(&pool).await;
    let repository = PostgresProtocolRoutingRepository::new(pool);
    committed(
        repository
            .change(&command(0, Some(selection.clone())))
            .await
            .unwrap(),
    );
    let context = channel(selection.clone());
    assert!(
        repository
            .resolve(&context, &claim())
            .await
            .unwrap()
            .is_some()
    );
    let mut wrong = context.clone();
    wrong.tenant_id = TenantId::new("tenant-b").unwrap();
    assert!(
        repository
            .resolve(&wrong, &claim())
            .await
            .unwrap()
            .is_none()
    );
    wrong = context.clone();
    wrong.channel_id = ProtocolChannelId::new("tcp-b").unwrap();
    assert!(
        repository
            .resolve(&wrong, &claim())
            .await
            .unwrap()
            .is_none()
    );
    wrong = context.clone();
    wrong.authority = Some(ExternalIdentifierAuthority::new("vendor").unwrap());
    assert!(
        repository
            .resolve(&wrong, &claim())
            .await
            .unwrap()
            .is_none()
    );
    wrong = context.clone();
    wrong.selection.world = ProtocolAdmissionWorld::Runtime;
    assert!(
        repository
            .resolve(&wrong, &claim())
            .await
            .unwrap()
            .is_none()
    );
    wrong = context.clone();
    wrong.selection.package = ProtocolPackageRef::new(
        selection.package.id().clone(),
        ProtocolPackageVersion::new("other").unwrap(),
    );
    assert!(
        repository
            .resolve(&wrong, &claim())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        repository
            .resolve(
                &context,
                &DeviceIdentityClaim::new("serial".into(), "unregistered".into()).unwrap()
            )
            .await
            .unwrap()
            .is_none()
    );
    let mut other = command(0, Some(selection));
    other.device_asset_id = AssetId::new("device-b").unwrap();
    committed(repository.change(&other).await.unwrap());
    let mut issuer = context;
    issuer.authority = Some(ExternalIdentifierAuthority::new("vendor").unwrap());
    assert_eq!(
        repository
            .resolve(&issuer, &claim())
            .await
            .unwrap()
            .unwrap()
            .route
            .device_asset_id,
        other.device_asset_id
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn session_revalidation_observes_changes_retirement_identifiers_and_suspension(pool: PgPool) {
    let selection = seed(&pool).await;
    let repository = PostgresProtocolRoutingRepository::new(pool.clone());
    committed(
        repository
            .change(&command(0, Some(selection.clone())))
            .await
            .unwrap(),
    );
    let service = ProtocolRoutingService::new(repository.clone());
    let context = channel(selection.clone());
    let session = service
        .resolve(context.clone(), claim())
        .await
        .unwrap()
        .unwrap();
    assert!(service.revalidate(&session).await.unwrap());
    committed(
        repository
            .change(&command(1, Some(selection.clone())))
            .await
            .unwrap(),
    );
    assert!(!service.revalidate(&session).await.unwrap());
    let fresh = service
        .resolve(context.clone(), claim())
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(!service.revalidate(&fresh).await.unwrap());
    assert_eq!(
        repository.change(&command(2, None)).await.unwrap(),
        ProtocolRouteWrite::Unavailable
    );
    sqlx::query("UPDATE metadata.tenants SET is_active=true WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    let resumed = service
        .resolve(context.clone(), claim())
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE metadata.asset_external_identifiers SET identifier_value='Changed' WHERE tenant_id='tenant-a' AND authority IS NULL").execute(&pool).await.unwrap();
    assert!(!service.revalidate(&resumed).await.unwrap());
    committed(repository.change(&command(2, None)).await.unwrap());
    sqlx::query("UPDATE metadata.asset_external_identifiers SET identifier_value='Device-001' WHERE tenant_id='tenant-a' AND authority IS NULL").execute(&pool).await.unwrap();
    assert!(
        service.resolve(context, claim()).await.unwrap().is_none(),
        "retirement denies even after the registered identifier is restored"
    );
    let retired = repository
        .find(
            &TenantId::new("tenant-a").unwrap(),
            &AssetId::new("device-a").unwrap(),
            &ProtocolChannelId::new("tcp-a").unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retired.revision.get(), 3);
    assert_eq!(retired.selection, None);
    assert_eq!(retired.policy_version, None);
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn stored_content_and_another_policy_receipt_do_not_imply_current_admission(pool: PgPool) {
    let selection = seed(&pool).await;
    let repository = PostgresProtocolRoutingRepository::new(pool.clone());
    let catalog = PostgresProtocolPackageCatalog::new(pool.clone());
    let store = PostgresProtocolArtifactStore::new(pool.clone());
    for (version, stored) in [("2", true), ("3", false)] {
        let artifact =
            ProtocolArtifact::new(format!("other-component-{version}").into_bytes()).unwrap();
        let package = ProtocolPackageRef::new(
            selection.package.id().clone(),
            ProtocolPackageVersion::new(version).unwrap(),
        );
        let release = ProtocolPackageRelease::new(
            TenantId::new("tenant-a").unwrap(),
            package.clone(),
            artifact.digest(),
            artifact.size_bytes(),
        );
        catalog.register(&release).await.unwrap();
        if stored {
            store.put(&release, &artifact).await.unwrap();
            let digest = artifact.digest();
            sqlx::query("INSERT INTO ingestion.protocol_admissions(tenant_id,protocol_package_id,protocol_package_version,artifact_sha256,wit_world,policy_version,actor_id,admitted_at_ms) VALUES ('tenant-a','custom.binary','2',$1,'identity-v1',2,'operator-a',1)")
                .bind(digest.as_bytes().as_slice()).execute(&pool).await.unwrap();
        }
        let request = command(
            0,
            Some(ProtocolRouteSelection {
                package,
                world: selection.world,
            }),
        );
        assert_eq!(
            repository.change(&request).await.unwrap(),
            ProtocolRouteWrite::Unavailable
        );
    }
}

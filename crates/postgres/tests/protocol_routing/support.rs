use super::*;
pub async fn seed(pool: &PgPool) -> ProtocolRouteSelection {
    sqlx::raw_sql("INSERT INTO metadata.tenants(tenant_id) VALUES ('tenant-a'),('tenant-b');
        INSERT INTO identity.actors(actor_id) VALUES ('operator-a'),('operator-b');
        INSERT INTO metadata.asset_type_definitions(tenant_id,type_id,version,display_name,property_count) VALUES ('tenant-a','equipment',1,'Equipment',0),('tenant-b','equipment',1,'Equipment',0);
        INSERT INTO metadata.assets(tenant_id,asset_id,type_id,type_version,display_name) VALUES ('tenant-a','device-a','equipment',1,'Device A'),('tenant-a','device-b','equipment',1,'Device B'),('tenant-a','plain-asset','equipment',1,'Asset'),('tenant-b','device-a','equipment',1,'Device A');
        INSERT INTO metadata.communication_devices(tenant_id,device_asset_id) VALUES ('tenant-a','device-a'),('tenant-a','device-b'),('tenant-b','device-a');
        INSERT INTO metadata.asset_external_identifiers(tenant_id,asset_id,identifier_kind,authority,identifier_value) VALUES ('tenant-a','device-a','serial',NULL,'Device-001'),('tenant-b','device-a','serial',NULL,'Device-001'),('tenant-a','device-b','serial','vendor','Device-001');")
        .execute(pool).await.unwrap();
    let selection = ProtocolRouteSelection {
        package: ProtocolPackageRef::new(
            ProtocolPackageId::new("custom.binary").unwrap(),
            ProtocolPackageVersion::new("1").unwrap(),
        ),
        world: ProtocolAdmissionWorld::Identity,
    };
    for tenant in ["tenant-a", "tenant-b"] {
        let artifact = ProtocolArtifact::new(b"fixture-component".to_vec()).unwrap();
        let release = ProtocolPackageRelease::new(
            TenantId::new(tenant).unwrap(),
            selection.package.clone(),
            artifact.digest(),
            artifact.size_bytes(),
        );
        PostgresProtocolPackageCatalog::new(pool.clone())
            .register(&release)
            .await
            .unwrap();
        let store = PostgresProtocolArtifactStore::new(pool.clone());
        store.put(&release, &artifact).await.unwrap();
        store
            .record(&ProtocolAdmission {
                release,
                world: selection.world,
                policy_version: PROTOCOL_ADMISSION_POLICY,
                actor_id: ActorId::new("operator-a").unwrap(),
                admitted_at: UnixTimeMillis::new(1),
            })
            .await
            .unwrap();
    }
    selection
}
pub fn command(expected: u32, selection: Option<ProtocolRouteSelection>) -> ProtocolRouteCommand {
    ProtocolRouteCommand {
        tenant_id: TenantId::new("tenant-a").unwrap(),
        device_asset_id: AssetId::new("device-a").unwrap(),
        channel_id: ProtocolChannelId::new("tcp-a").unwrap(),
        expected_revision: expected,
        selection,
        actor_id: ActorId::new("operator-a").unwrap(),
        recorded_at: UnixTimeMillis::new(100),
    }
}
pub fn channel(selection: ProtocolRouteSelection) -> RegisteredIdentityChannel {
    RegisteredIdentityChannel {
        tenant_id: TenantId::new("tenant-a").unwrap(),
        channel_id: ProtocolChannelId::new("tcp-a").unwrap(),
        authority: None,
        selection,
    }
}
pub fn claim() -> DeviceIdentityClaim {
    DeviceIdentityClaim::new("serial".into(), "Device-001".into()).unwrap()
}
pub fn committed(outcome: ProtocolRouteWrite) -> ProtocolDeviceRoute {
    let ProtocolRouteWrite::Committed(route) = outcome else {
        panic!("expected a committed route")
    };
    route
}

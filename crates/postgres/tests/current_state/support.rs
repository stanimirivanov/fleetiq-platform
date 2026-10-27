use fleetiq_domain::{
    AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, SignalTarget,
};
use fleetiq_postgres::PostgresCurrentStateRepository;
use fleetiq_projection::{ProjectionMaintenance, ProjectionTick};
use sqlx::PgPool;
use std::error::Error;
pub(super) const INSERT: &str = "INSERT INTO history.asset_property_changes (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis,event_at_ms,resolution_effective_at_ms,resolution_known_at_ms) VALUES ('tenant-a','message-1','vendor.protocol','1',$1,'binding-1',1,$2,$3,1000,150) ON CONFLICT DO NOTHING";
pub(super) async fn setup(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(include_str!("../property_history/topology.sql"))
        .execute(pool)
        .await?;
    Ok(())
}
pub(super) async fn append(
    pool: &PgPool,
    output: i64,
    basis: &str,
    event: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(INSERT)
        .bind(output)
        .bind(basis)
        .bind(event)
        .execute(pool)
        .await?;
    Ok(())
}
pub(super) fn target(property: &str, version: u32) -> Result<SignalTarget, Box<dyn Error>> {
    Ok(SignalTarget::new(
        AssetId::new("asset-a")?,
        PropertyDefinitionRef::new(
            PropertyDefinitionId::new(property)?,
            DefinitionVersion::new(version)?,
        ),
    ))
}
pub(super) async fn drain(
    repository: &PostgresCurrentStateRepository,
) -> Result<(), Box<dyn Error>> {
    for _ in 0..5 {
        if repository.tick().await? == ProjectionTick::Idle {
            return Ok(());
        }
    }
    Err("projection did not drain within fixture bound".into())
}

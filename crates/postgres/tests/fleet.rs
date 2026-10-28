//! Consistent bounded pages, aggregates and source-group ambiguity on real PostgreSQL.
use fleetiq_domain::{
    AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, TenantId,
    UnixTimeMillis,
};
use fleetiq_postgres::PostgresCurrentStateRepository;
use fleetiq_projection::{
    CoordinateEncoding, FleetQuery, FleetReadRepository, LocationMapping, LocationState,
    ProjectionMaintenance, classify_location,
};
use sqlx::PgPool;
use std::error::Error;
fn query(limit: u16, after: Option<AssetId>, encoding: CoordinateEncoding) -> FleetQuery {
    let property = |id| {
        PropertyDefinitionRef::new(
            PropertyDefinitionId::new(id).unwrap(),
            DefinitionVersion::new(1).unwrap(),
        )
    };
    FleetQuery::new(
        LocationMapping::new(
            property("position.latitude"),
            property("position.longitude"),
            encoding,
        )
        .unwrap(),
        limit,
        after,
    )
    .unwrap()
}
async fn setup(
    pool: &PgPool,
    ambiguous: bool,
) -> Result<PostgresCurrentStateRepository, Box<dyn Error>> {
    sqlx::raw_sql(include_str!("property_history/topology.sql"))
        .execute(pool)
        .await?;
    let fixture = if ambiguous {
        include_str!("fleet/ambiguous-topology.sql")
    } else {
        include_str!("fleet/topology.sql")
    };
    sqlx::raw_sql(fixture).execute(pool).await?;
    let repository = PostgresCurrentStateRepository::new(pool.clone());
    repository.tick().await?;
    Ok(repository)
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn pages_preserve_unlocated_assets_and_whole_tenant_counts(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let repository = setup(&pool, false).await?;
    let tenant = TenantId::new("tenant-a")?;
    let mut after = None;
    let mut ids = Vec::new();
    loop {
        let q = query(1, after, CoordinateEncoding::E7);
        let page = repository.fleet(&tenant, &q).await?.unwrap();
        assert_eq!(page.summary.assets, 5);
        assert_eq!(page.summary.communication_devices, 1);
        assert_eq!(page.summary.mapped_assets, 2);
        assert_eq!(page.summary.assets_with_current_properties, 2);
        assert!(page.mapping_supported);
        assert_eq!(page.assets.len(), 1);
        ids.push(page.assets[0].id.as_str().to_owned());
        if page.assets[0].id.as_str() == "located-a" {
            assert!(matches!(
                classify_location(
                    &page.assets[0],
                    q.mapping(),
                    true,
                    UnixTimeMillis::new(2000)
                ),
                LocationState::Observed(_)
            ));
        }
        if page.next_after.is_none() {
            break;
        }
        after = page.next_after;
    }
    assert_eq!(
        ids,
        ["asset-a", "asset-b", "gateway-1", "located-a", "located-b"]
    );
    let page = repository
        .fleet(
            &tenant,
            &query(1, Some(AssetId::new("zz")?), CoordinateEncoding::E7),
        )
        .await?
        .unwrap();
    assert_eq!(page.assets.len(), 0);
    assert_eq!(page.summary.assets, 5);
    assert_eq!(page.next_after, None);
    let other = repository
        .fleet(
            &TenantId::new("tenant-b")?,
            &query(100, None, CoordinateEncoding::E7),
        )
        .await?
        .unwrap();
    assert_eq!(other.summary.assets, 0);
    assert_eq!(other.assets.len(), 0);
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn repeated_source_device_time_is_not_a_coordinate_pair(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let repository = setup(&pool, true).await?;
    let q = query(100, None, CoordinateEncoding::E7);
    let page = repository
        .fleet(&TenantId::new("tenant-a")?, &q)
        .await?
        .unwrap();
    let asset = page
        .assets
        .iter()
        .find(|a| a.id.as_str() == "located-a")
        .unwrap();
    assert!(asset.ambiguous);
    assert_eq!(
        classify_location(asset, q.mapping(), true, UnixTimeMillis::new(2000)),
        LocationState::Incoherent
    );
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn unitless_degrees_configuration_remains_unsupported(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let repository = setup(&pool, false).await?;
    let page = repository
        .fleet(
            &TenantId::new("tenant-a")?,
            &query(100, None, CoordinateEncoding::Degrees),
        )
        .await?
        .unwrap();
    assert!(!page.mapping_supported);
    assert_eq!(page.summary.assets, 5);
    assert!(
        page.assets
            .iter()
            .all(|a| a.latitude.is_none() && a.longitude.is_none())
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn a_supported_axis_cannot_hide_the_other_axis_null_unit(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let repository = setup(&pool, false).await?;
    sqlx::query("INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind,canonical_unit) VALUES ('tenant-a','position.longitude',2,'Longitude degrees','integer','deg')").execute(&pool).await?;
    let property = |id, version| {
        PropertyDefinitionRef::new(
            PropertyDefinitionId::new(id).unwrap(),
            DefinitionVersion::new(version).unwrap(),
        )
    };
    let mapping = LocationMapping::new(
        property("position.latitude", 1),
        property("position.longitude", 2),
        CoordinateEncoding::Degrees,
    )?;
    let page = repository
        .fleet(
            &TenantId::new("tenant-a")?,
            &FleetQuery::new(mapping, 100, None)?,
        )
        .await?
        .unwrap();
    assert!(!page.mapping_supported);
    assert_eq!(page.summary.mapped_assets, 0);
    Ok(())
}

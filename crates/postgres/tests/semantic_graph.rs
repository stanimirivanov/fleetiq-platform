//! Bounded real PostgreSQL graph reads with active temporal/integrity constraints.
#[path = "semantic_graph/support.rs"]
mod support;
use fleetiq_domain::{AssetId, TenantId};
use fleetiq_metadata::{
    GraphBudget, GraphTruncation, SemanticGraphRead, SemanticGraphRepository, SemanticGraphService,
};
use fleetiq_postgres::{PostgresRepositoryError, PostgresSemanticGraphRepository};
use sqlx::PgPool;
use std::error::Error;
use support::{grant, query, setup};

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn cycles_bindings_and_self_links_have_unique_closed_tenant_owned_endpoints(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool).await?;
    let service = SemanticGraphService::new(PostgresSemanticGraphRepository::new(pool.clone()));
    let graph = service
        .explore(
            grant(&pool, "tenant-a").await?,
            &query(50, 50, GraphBudget::default()),
        )
        .await?
        .unwrap();
    assert_eq!(graph.assets().len(), 4);
    assert_eq!(graph.relationships().len(), 4);
    assert_eq!(graph.bindings().len(), 2);
    assert_eq!(graph.truncation(), []);
    for edge in graph.relationships() {
        assert!(
            graph
                .assets()
                .iter()
                .any(|a| a.id() == edge.source_asset_id())
                && graph
                    .assets()
                    .iter()
                    .any(|a| a.id() == edge.target_asset_id())
        );
    }
    for edge in graph.bindings() {
        assert!(
            graph
                .assets()
                .iter()
                .any(|a| a.id() == edge.source().device_asset_id())
                && graph
                    .assets()
                    .iter()
                    .any(|a| a.id() == edge.target().asset_id())
        );
    }
    sqlx::query("INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-b','operator-a','metadata.read')").execute(&pool).await?;
    let foreign = service
        .explore(
            grant(&pool, "tenant-b").await?,
            &query(50, 50, GraphBudget::default()),
        )
        .await?
        .unwrap();
    assert_eq!(foreign.assets().len(), 1);
    assert_eq!(foreign.assets()[0].name().as_str(), "Other tenant asset");
    assert_eq!(foreign.relationships(), []);
    assert_eq!(foreign.bindings(), []);
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn latest_known_corrections_are_applied_before_endpoint_and_effective_filters(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool).await?;
    sqlx::raw_sql("UPDATE metadata.asset_relationships SET latest_revision=2 WHERE tenant_id='tenant-a' AND relationship_id='ab';
        INSERT INTO metadata.asset_relationship_revisions (tenant_id,relationship_id,revision,recorded_at_ms,relationship_type_id,relationship_type_version,source_asset_id,target_asset_id,effective_start_ms,effective_end_ms) VALUES ('tenant-a','ab',2,40,'connected',1,'asset-d','detached',0,NULL);
        UPDATE metadata.signal_bindings SET latest_revision=2 WHERE tenant_id='tenant-a' AND binding_id='bind-1';
        INSERT INTO metadata.signal_binding_revisions (tenant_id,binding_id,revision,recorded_at_ms,source_device_asset_id,source_endpoint_id,source_signal_id,target_asset_id,target_property_id,target_property_version,effective_start_ms,effective_end_ms) VALUES ('tenant-a','bind-1',2,40,'gateway-2','can','bus.voltage','asset-d','voltage',1,0,NULL);").execute(&pool).await?;
    let service = SemanticGraphService::new(PostgresSemanticGraphRepository::new(pool.clone()));
    let before = service
        .explore(
            grant(&pool, "tenant-a").await?,
            &query(50, 30, GraphBudget::default()),
        )
        .await?
        .unwrap();
    assert!(
        before
            .relationships()
            .iter()
            .any(|r| r.id().as_str() == "ab")
    );
    assert!(
        before
            .bindings()
            .iter()
            .any(|b| b.id().as_str() == "bind-1")
    );
    let after = service
        .explore(
            grant(&pool, "tenant-a").await?,
            &query(50, 50, GraphBudget::default()),
        )
        .await?
        .unwrap();
    assert!(
        !after
            .relationships()
            .iter()
            .any(|r| r.id().as_str() == "ab")
    );
    assert_eq!(after.bindings(), []);
    assert!(
        !after
            .assets()
            .iter()
            .any(|a| a.id().as_str().starts_with("gateway"))
    );
    // At the old knowledge cutoff, exact half-open end excludes ab/bind-1.
    let ended = service
        .explore(
            grant(&pool, "tenant-a").await?,
            &query(100, 30, GraphBudget::default()),
        )
        .await?
        .unwrap();
    assert!(
        !ended
            .relationships()
            .iter()
            .any(|r| r.id().as_str() == "ab")
    );
    assert_eq!(ended.bindings(), []);
    let early = service
        .explore(
            grant(&pool, "tenant-a").await?,
            &query(-1, 50, GraphBudget::default()),
        )
        .await?
        .unwrap();
    assert_eq!(early.assets().len(), 1);
    assert_eq!(early.relationships(), []);
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn high_fan_out_and_depth_budgets_report_incompleteness_without_dangling_edges(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool).await?;
    sqlx::raw_sql("INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name) SELECT 'tenant-a','fan-'||lpad(n::text,3,'0'),'equipment',1,'Fan '||n::text FROM generate_series(1,110) AS n;
        INSERT INTO metadata.asset_relationships (tenant_id,relationship_id,latest_revision) SELECT 'tenant-a','fan-'||lpad(n::text,3,'0'),1 FROM generate_series(1,110) AS n;
        INSERT INTO metadata.asset_relationship_revisions (tenant_id,relationship_id,revision,recorded_at_ms,relationship_type_id,relationship_type_version,source_asset_id,target_asset_id,effective_start_ms,effective_end_ms) SELECT 'tenant-a','fan-'||lpad(n::text,3,'0'),1,10,'connected',1,'asset-a','fan-'||lpad(n::text,3,'0'),0,NULL FROM generate_series(1,110) AS n;").execute(&pool).await?;
    let service = SemanticGraphService::new(PostgresSemanticGraphRepository::new(pool.clone()));
    let q = query(50, 50, GraphBudget::new(4, 100, 200, 200)?);
    let graph = service
        .explore(grant(&pool, "tenant-a").await?, &q)
        .await?
        .unwrap();
    assert_eq!(graph.assets().len(), 100);
    assert!(graph.truncation().contains(&GraphTruncation::Assets));
    for edge in graph.relationships() {
        assert!(
            graph
                .assets()
                .iter()
                .any(|a| a.id() == edge.target_asset_id())
                && graph
                    .assets()
                    .iter()
                    .any(|a| a.id() == edge.source_asset_id())
        );
    }
    assert_eq!(
        graph,
        service
            .explore(grant(&pool, "tenant-a").await?, &q)
            .await?
            .unwrap()
    );
    let graph = service
        .explore(
            grant(&pool, "tenant-a").await?,
            &query(50, 50, GraphBudget::new(0, 100, 200, 200)?),
        )
        .await?
        .unwrap();
    assert_eq!(graph.assets().len(), 1);
    assert_eq!(graph.relationships().len(), 1);
    assert!(graph.truncation().contains(&GraphTruncation::Depth));
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn every_frontier_uses_the_same_repeatable_read_snapshot(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool).await?;
    let repository = PostgresSemanticGraphRepository::new(pool.clone());
    let tenant = TenantId::new("tenant-a")?;
    let q = query(50, 50, GraphBudget::default());
    let mut first = repository.open(&tenant, &q).await?;
    let root = [AssetId::new("asset-a")?];
    assert_eq!(first.assets(&root).await?.len(), 1);
    // A committed backdated revision passes the fixed cutoff but must not enter
    // the already-open snapshot. No sleeps or process scheduling assumptions.
    sqlx::raw_sql("INSERT INTO metadata.asset_relationships (tenant_id,relationship_id,latest_revision) VALUES ('tenant-a','late',1);
        INSERT INTO metadata.asset_relationship_revisions (tenant_id,relationship_id,revision,recorded_at_ms,relationship_type_id,relationship_type_version,source_asset_id,target_asset_id,effective_start_ms,effective_end_ms) VALUES ('tenant-a','late',1,20,'connected',1,'asset-a','detached',0,NULL);").execute(&pool).await?;
    assert!(
        !first
            .relationships(&root, &[], 201)
            .await?
            .iter()
            .any(|r| r.id().as_str() == "late")
    );
    let mut fresh = repository.open(&tenant, &q).await?;
    assert!(
        fresh
            .relationships(&root, &[], 201)
            .await?
            .iter()
            .any(|r| r.id().as_str() == "late")
    );
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn blocked_snapshot_read_times_out_instead_of_returning_an_empty_graph(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool).await?;
    let mut blocker = pool.begin().await?;
    sqlx::query("LOCK TABLE metadata.assets IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocker)
        .await?;
    let repository = PostgresSemanticGraphRepository::new(pool.clone());
    let mut read = repository
        .open(
            &TenantId::new("tenant-a")?,
            &query(50, 50, GraphBudget::default()),
        )
        .await?;
    let error = read.assets(&[AssetId::new("asset-a")?]).await.unwrap_err();
    let PostgresRepositoryError::Database(sqlx::Error::Database(error)) = error else {
        return Err("expected a query timeout".into());
    };
    assert_eq!(error.code().as_deref(), Some("57014"));
    blocker.rollback().await?;
    Ok(())
}

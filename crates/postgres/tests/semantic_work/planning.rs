//! Exercise the actual prepared query and a deliberately unseekable control.
use sqlx::{PgPool, Postgres, Transaction};
use std::error::Error;

async fn has_source_range(
    tx: &mut Transaction<'_, Postgres>,
    query: &'static str,
) -> Result<bool, sqlx::Error> {
    let plan: Vec<String> = sqlx::query_scalar(query).fetch_all(&mut **tx).await?;
    Ok(plan.iter().any(|line| {
        line.contains("Index Cond:")
            && line.contains("protocol_package_version")
            && line.contains("output_index")
    }))
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn continuing_backfill_is_an_indexable_range_not_a_join_filter(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let mut tx = pool.begin().await?;
    // Establish index eligibility independent of tiny-fixture cost estimates.
    // These planner switches are test-local, never production configuration.
    sqlx::query("SET LOCAL enable_seqscan=off")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL enable_bitmapscan=off")
        .execute(&mut *tx)
        .await?;
    let old = r"EXPLAIN (COSTS OFF) WITH cursor AS MATERIALIZED (
        SELECT * FROM history.semantic_backfill WHERE NOT completed FOR UPDATE SKIP LOCKED
    ) SELECT o.tenant_id,o.message_id,o.protocol_package_id,o.protocol_package_version,o.output_index
      FROM ingestion.decoded_observations o,cursor c
      WHERE c.tenant_id IS NULL OR
          (o.tenant_id,o.message_id,o.protocol_package_id,o.protocol_package_version,o.output_index) >
          (c.tenant_id,c.message_id,c.protocol_package_id,c.protocol_package_version,c.output_index)
      ORDER BY o.tenant_id,o.message_id,o.protocol_package_id,o.protocol_package_version,o.output_index
      LIMIT 32";
    assert!(
        !has_source_range(&mut tx, old).await?,
        "negative control must reject a full-index join filter"
    );
    assert!(
        has_source_range(
            &mut tx,
            concat!(
                "EXPLAIN (COSTS OFF) ",
                include_str!("../../src/history/work/seed.sql")
            )
        )
        .await?,
        "continuing source keys must be eligible for an indexed seek"
    );
    tx.rollback().await?;
    Ok(())
}

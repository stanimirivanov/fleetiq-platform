//! Synthetic retained-history measurements of the production SQL selector.
use super::*;

#[test]
fn plan_observations_keep_cost_evidence_and_drop_query_contents() {
    let private = "private-plan-predicate";
    let plan = json!([{
        "Planning Time": 0.4, "Execution Time": 1.2,
        "Plan": {
            "Node Type": "Limit", "Actual Rows": 1, "Actual Loops": 1,
            "Shared Hit Blocks": 12, "Shared Read Blocks": 3,
            "Filter": private,
            "Plans": [{
                "Node Type": "Index Scan", "Actual Rows": 100, "Actual Loops": 2,
                "Rows Removed by Filter": 99, "Index Cond": private
            }]
        }
    }]);
    let observation = summarize_plan(&plan).unwrap();
    assert_eq!(observation["execution_ms"], 1.2);
    assert_eq!(observation["shared_hit_blocks"], 12);
    assert_eq!(observation["shared_read_blocks"], 3);
    assert_eq!(observation["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(observation["nodes"][1]["rows"], 100);
    assert_eq!(observation["nodes"][1]["loops"], 2);
    assert_eq!(observation["nodes"][1]["removed_by_filter"], 99);
    assert!(!observation.to_string().contains(private));
    assert!(summarize_plan(&json!([])).is_err());
}

fn summarize_plan(document: &Value) -> Result<Value, Box<dyn Error>> {
    let plan = document[0]["Plan"]
        .as_object()
        .ok_or("Missing analyzed plan")?;
    let execution = document[0]["Execution Time"]
        .as_f64()
        .ok_or("Missing execution time")?;
    let mut nodes = Vec::new();
    collect_nodes(&document[0]["Plan"], &mut nodes);
    Ok(json!({
        "planning_ms": document[0]["Planning Time"],
        "execution_ms": execution,
        "shared_hit_blocks": plan.get("Shared Hit Blocks"),
        "shared_read_blocks": plan.get("Shared Read Blocks"),
        "temp_read_blocks": plan.get("Temp Read Blocks"),
        "temp_written_blocks": plan.get("Temp Written Blocks"),
        "nodes": nodes,
    }))
}

fn collect_nodes(node: &Value, nodes: &mut Vec<Value>) {
    nodes.push(json!({
        "type": node["Node Type"], "rows": node["Actual Rows"],
        "loops": node["Actual Loops"],
        "removed_by_filter": node["Rows Removed by Filter"],
        "removed_by_join_filter": node["Rows Removed by Join Filter"],
    }));
    if let Some(children) = node["Plans"].as_array() {
        for child in children {
            collect_nodes(child, nodes);
        }
    }
}

async fn observe(pool: &PgPool, expected: Option<&str>) -> Result<Value, Box<dyn Error>> {
    use std::time::Instant;
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL statement_timeout = '2s'")
        .execute(&mut *tx)
        .await?;
    let mut latency = Vec::new();
    for _ in 0..5 {
        let started = Instant::now();
        let selected: Option<(String, bool)> = sqlx::query_as(
            "SELECT evaluation_id,scheduled FROM analytics.next_alert_evidence('tenant-a','high',1)",
        ).fetch_optional(&mut *tx).await?;
        latency.push(started.elapsed().as_micros());
        assert_eq!(selected.as_ref().map(|row| row.0.as_str()), expected);
        if let Some((_, scheduled)) = selected {
            assert!(
                scheduled,
                "Pending probe must retain completed scheduled work evidence"
            );
        }
    }
    let plan: Value = sqlx::query_scalar(
        "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)
         SELECT * FROM analytics.next_alert_evidence('tenant-a','high',1)",
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    latency.sort_unstable();
    Ok(json!({
        "selected_rows": usize::from(expected.is_some()),
        "samples": latency.len(),
        "latency_us": {"min":latency[0],"p50":latency[2],"max":latency[4]},
        "plan": summarize_plan(&plan)?,
    }))
}

async fn retain_history(
    pool: &PgPool,
    from: i64,
    through: i64,
    base: i64,
) -> Result<(), Box<dyn Error>> {
    // Bulk-load valid immutable historical envelopes and their historical receipts.
    // A different scope contributes unconsumed noise without changing the rule.
    sqlx::query(
        "WITH source AS (
           SELECT 'retained-'||g AS id,jsonb_build_object(
             'scope','asset-a','calculator','mean','version','1','configuration','history',
             'start',$3::bigint-g*1000-1000,'end',$3::bigint-g*1000,
             'finalize',$3::bigint-g*1000,'known',$3::bigint-g*1000,'inputs','[]'::jsonb) AS k
           FROM generate_series($1::bigint,$2::bigint) g
           UNION ALL
           SELECT 'noise-'||g,jsonb_build_object(
             'scope','other','calculator','mean','version','1','configuration','noise',
             'start',$3::bigint-g*1000-1000,'end',$3::bigint-g*1000,
             'finalize',$3::bigint-g*1000,'known',$3::bigint-g*1000,'inputs','[]'::jsonb)
           FROM generate_series($1::bigint,$2::bigint) g
         )
         INSERT INTO analytics.evaluations
           (tenant_id,evaluation_id,replay_key,key_digest,minimum_samples,evaluated_at_ms,outcome,outputs)
         SELECT 'tenant-a',id,k,sha256(convert_to(k::text,'UTF8')),1,(k->>'known')::bigint,'insufficient','[]'
         FROM source",
    ).bind(from).bind(through).bind(base).execute(pool).await?;
    sqlx::query(
        "INSERT INTO analytics.alert_evaluation_receipts
         (tenant_id,rule_id,revision,evaluation_id,activation_generation,disposition)
         SELECT 'tenant-a','high',1,'retained-'||g,1,'historical'
         FROM generate_series($1::bigint,$2::bigint) g",
    )
    .bind(from)
    .bind(through)
    .execute(pool)
    .await?;
    sqlx::raw_sql("ANALYZE analytics.evaluations; ANALYZE analytics.alert_evaluation_receipts; ANALYZE analytics.calculator_work;")
        .execute(pool).await?;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn measured_retained_history_selection(pool: PgPool) -> Result<(), Box<dyn Error>> {
    use std::time::{Duration, Instant};
    let base = setup(&pool, 0, 0).await?;
    let worker = PostgresAlertLifecycle::new(pool.clone());
    let mut observations = Vec::new();
    let mut previous = 0;
    for retained in [0, 256, 4096, 32768] {
        retain_history(&pool, previous + 1, retained, base).await?;
        let counts_before = counts(&pool).await;
        let exhausted = observe(&pool, None).await?;
        assert_eq!(counts(&pool).await, counts_before);
        let started = Instant::now();
        assert!(!tokio::time::timeout(Duration::from_secs(15), worker.advance_one()).await??);
        let idle_turn_us = started.elapsed().as_micros();

        let id = format!("pending-{retained}");
        append(&pool, &id, base + 1_000_000 + retained * 1000, Some("51")).await?;
        let pending = observe(&pool, Some(&id)).await?;
        assert_eq!(counts(&pool).await, counts_before);
        let started = Instant::now();
        assert!(tokio::time::timeout(Duration::from_secs(15), worker.advance_one()).await??);
        let pending_turn_us = started.elapsed().as_micros();
        assert_eq!(counts(&pool).await.0, counts_before.0 + 1);
        observations.push(json!({
            "bulk_receipted_history":retained,"unconsumed_other_scope":retained,
            "receipts_before":counts_before.0,
            "fully_receipted":exhausted,"pending":pending,
            "worker_idle_turn_us":idle_turn_us,"worker_pending_turn_us":pending_turn_us,
        }));
        previous = retained;
    }
    // New commits with earlier event ends must survive retained-prefix exclusion.
    // Equal ends use C-collated IDs, independently of insertion order.
    let before = counts(&pool).await;
    for id in ["late-z", "late-a"] {
        append_live(
            &pool,
            &record(id, base + 10, Some("51"), "result", id),
            "result",
        )
        .await?;
    }
    for id in ["late-a", "late-z"] {
        observe(&pool, Some(id)).await?;
        assert!(worker.advance_one().await?);
        let disposition: String = sqlx::query_scalar(
            "SELECT disposition FROM analytics.alert_evaluation_receipts WHERE evaluation_id=$1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(disposition, "out_of_order");
    }
    assert_eq!(counts(&pool).await, (before.0 + 2, before.1, before.2));
    observe(&pool, None).await?;

    let version: String = sqlx::query_scalar("SHOW server_version")
        .fetch_one(&pool)
        .await?;
    let report = json!({
        "schema":"fleetiq.alert-selection-measurement.v1",
        "postgres_version":version,"observations":observations,
        "correctness":{"late_commits":2,"equal_end_id_order":true,"read_only_observations":true},
        "limits":"warm synthetic local fixture; timings are observations, not calibrated SLAs; selector statements use a two-second deadline",
    });
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/alert-selection-measurements");
    std::fs::create_dir_all(&directory)?;
    std::fs::write(
        directory.join(format!("selection-{}.json", std::process::id())),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("ALERT_SELECTION_MEASUREMENT {report}");
    Ok(())
}

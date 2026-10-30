//! Real PostgreSQL lifecycle, replay, selection, cancellation and outbox guarantees.
use fleetiq_analytics::*;
use fleetiq_domain::*;
use fleetiq_postgres::{
    PostgresAlertLifecycle, PostgresCalculatorEvaluationStore, PostgresCalculatorWork,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::error::Error;
fn definition(hold: u32, resolve: u32) -> Value {
    json!({"kind":"numeric-threshold-v1","calculator":"mean","version":"1","scope":"asset-a","target":"result","output_property":"mean.voltage","output_version":1,"output_index":0,"comparison":"gt","threshold":"50","threshold_unit":"volt","hold_ms":hold,"resolution":"resolve_on_false","resolve_after_ms":resolve})
}
async fn setup(pool: &PgPool, hold: u32, resolve: u32) -> Result<i64, Box<dyn Error>> {
    sqlx::raw_sql(include_str!("property_history/topology.sql"))
        .execute(pool)
        .await?;
    sqlx::raw_sql("INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-a','alerts.manage');
      INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind,canonical_unit) VALUES ('tenant-a','mean.voltage',1,'Mean','decimal','volt');
      INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count) VALUES ('tenant-a','calculated',1,'Calculated',1);
      INSERT INTO metadata.asset_type_properties (tenant_id,type_id,type_version,property_id,property_version,property_ordinal) VALUES ('tenant-a','calculated',1,'mean.voltage',1,1);
      INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name) VALUES ('tenant-a','result','calculated',1,'Result'),('tenant-a','other','calculated',1,'Other');
      INSERT INTO analytics.calculator_definitions VALUES ('tenant-a','mean','1','window-mean-v1','electrical.voltage',1,'mean.voltage',1);").execute(pool).await?;
    sqlx::query("SELECT analytics.create_alert_rule_revision('tenant-a','high',1,'operator-a',$1)")
        .bind(definition(hold, resolve))
        .execute(pool)
        .await?;
    sqlx::query(
        "SELECT analytics.change_alert_rule_activation('tenant-a','high',0,1,'operator-a')",
    )
    .execute(pool)
    .await?;
    Ok(sqlx::query_scalar("SELECT recorded_at_ms FROM analytics.alert_rule_activation_changes WHERE tenant_id='tenant-a' AND rule_id='high' AND generation=1").fetch_one(pool).await?)
}
fn record(
    id: &str,
    start: i64,
    value: Option<&str>,
    target: &str,
    configuration: &str,
) -> CalculatorEvaluationRecord {
    let tenant = TenantId::new("tenant-a").unwrap();
    let scope = AssetId::new("asset-a").unwrap();
    let release = CalculatorRef::new(
        CalculatorId::new("mean").unwrap(),
        CalculatorVersion::new("1").unwrap(),
    );
    let end = UnixTimeMillis::new(start + 1000);
    let evaluation = CalculatorEvaluation::new(
        CalculatorEvaluationId::new(id).unwrap(),
        tenant.clone(),
        scope.clone(),
        release.clone(),
        end,
        end,
    );
    let key = CalculatorExecutionKey::new(
        tenant,
        scope,
        release,
        CalculatorConfigurationId::new(configuration).unwrap(),
        CalculatorInputManifest::new(
            CalculatorWindow::new(UnixTimeMillis::new(start), end, end).unwrap(),
            end,
            vec![],
        )
        .unwrap(),
    );
    let property = PropertyDefinition::new(
        PropertyDefinitionRef::new(
            PropertyDefinitionId::new("mean.voltage").unwrap(),
            DefinitionVersion::new(1).unwrap(),
        ),
        DefinitionName::new("Mean").unwrap(),
        PropertyValueKind::Decimal,
        Some(UnitId::new("volt").unwrap()),
    )
    .unwrap();
    let outputs = assemble_outputs(
        &evaluation,
        value.into_iter().map(|value| {
            CalculatedProperty::new(
                AssetId::new(target).unwrap(),
                &property,
                PropertyValue::Decimal(value.parse().unwrap()),
                Some(UnitId::new("volt").unwrap()),
            )
            .unwrap()
        }),
    )
    .unwrap();
    CalculatorEvaluationRecord::new(
        key,
        evaluation,
        if value.is_some() {
            CalculatorExecutionOutcome::Complete
        } else {
            CalculatorExecutionOutcome::InsufficientEvidence
        },
        outputs,
        1,
    )
    .unwrap()
}
async fn append_live(
    pool: &PgPool,
    record: &CalculatorEvaluationRecord,
    target: &str,
) -> Result<(), Box<dyn Error>> {
    let evaluation = record.evaluation();
    let key = record.key();
    let window = key.manifest().window();
    let request = CalculatorRunRequest {
        evaluation: evaluation.clone(),
        configuration: key.configuration().clone(),
        window,
        known_at: key.manifest().known_at(),
        minimum_samples: 1,
        target: AssetId::new(target)?,
    };
    let work = PostgresCalculatorWork::new(pool.clone());
    work.register(&CalculatorSchedule {
        tenant: evaluation.tenant_id().clone(),
        id: CalculatorConfigurationId::new(evaluation.id().as_str())?,
        calculator: evaluation.calculator().clone(),
        configuration: request.configuration.clone(),
        scope: evaluation.scope_asset_id().clone(),
        target: request.target.clone(),
        first_start: window.start(),
        window_ms: 1000,
        lateness_ms: 0,
        minimum_samples: 1,
    })
    .await?;
    let json = json!({"scope":evaluation.scope_asset_id().as_str(),"calculator":"mean","version":"1","configuration":request.configuration.as_str(),
        "start":window.start().get(),"end":window.end().get(),"finalize":window.finalize_at().get(),"known":request.known_at.get(),
        "evaluated":evaluation.evaluated_at().get(),"minimum":1,"target":target});
    // Simulate an issued unexpired claim; complete exercises the production atomic
    // append/status boundary that identifies scheduled (rather than one-shot) work.
    sqlx::query("INSERT INTO analytics.calculator_work(tenant_id,evaluation_id,schedule_id,start_ms,request,generation,lease_until_ms) VALUES('tenant-a',$1,$1,$2,$3,1,floor(extract(epoch FROM clock_timestamp())*1000)::bigint+30000)")
        .bind(evaluation.id().as_str()).bind(window.start().get()).bind(json).execute(pool).await?;
    work.complete(
        &CalculatorWorkLease {
            request,
            generation: 1,
        },
        record,
    )
    .await?;
    Ok(())
}
async fn append(
    pool: &PgPool,
    id: &str,
    start: i64,
    value: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    append_live(pool, &record(id, start, value, "result", "c1"), "result").await
}
async fn phase(pool: &PgPool) -> String {
    sqlx::query_scalar("SELECT phase FROM analytics.alert_lifecycles WHERE tenant_id='tenant-a' AND rule_id='high' AND revision=1").fetch_one(pool).await.unwrap()
}
async fn counts(pool: &PgPool) -> (i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM analytics.alert_evaluation_receipts),(SELECT count(*) FROM analytics.alert_transitions),(SELECT count(*) FROM analytics.alert_notification_intents)").fetch_one(pool).await.unwrap()
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn pending_firing_resolution_restart_and_duplicate_retries_are_atomic(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let base = setup(&pool, 1000, 1000).await?;
    let worker = PostgresAlertLifecycle::new(pool.clone());
    for (id, start, value, expected) in [
        ("e1", base, "51", "pending"),
        ("e2", base + 1000, "52", "firing"),
        ("e3", base + 2000, "49", "firing"),
        ("e4", base + 3000, "48", "resolved"),
    ] {
        append(&pool, id, start, Some(value)).await?;
        assert!(worker.advance_one().await?);
        assert_eq!(phase(&pool).await, expected);
    }
    assert_eq!(counts(&pool).await, (4, 3, 2));
    PostgresCalculatorEvaluationStore::new(pool.clone())
        .append(&record("e4", base + 3000, Some("48"), "result", "c1"))
        .await?;
    assert!(
        !PostgresAlertLifecycle::new(pool.clone())
            .advance_one()
            .await?
    );
    assert_eq!(counts(&pool).await, (4, 3, 2));
    let phases: Vec<String> =
        sqlx::query_scalar("SELECT phase FROM analytics.alert_transitions ORDER BY version")
            .fetch_all(&pool)
            .await?;
    assert_eq!(phases, ["pending", "firing", "resolved"]);
    let scope:String=sqlx::query_scalar("SELECT r.scope_asset_id FROM analytics.alert_lifecycles a JOIN analytics.alert_rule_revisions r USING(tenant_id,rule_id,revision)").fetch_one(&pool).await?;
    assert_eq!(scope, "asset-a");
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn missing_and_window_gaps_interrupt_timers_but_never_resolve_firing(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let base = setup(&pool, 1000, 1000).await?;
    let worker = PostgresAlertLifecycle::new(pool.clone());
    for (id, offset, value, expected) in [
        ("p", 0, Some("51"), "pending"),
        ("missing", 1000, None, "inactive"),
        ("p2", 2000, Some("51"), "pending"),
        ("gap", 4000, Some("51"), "pending"),
        ("fire", 5000, Some("51"), "firing"),
        ("false", 6000, Some("49"), "firing"),
        ("unknown", 7000, None, "firing"),
        ("false2", 8000, Some("49"), "firing"),
        ("resolve", 9000, Some("49"), "resolved"),
    ] {
        append(&pool, id, base + offset, value).await?;
        assert!(worker.advance_one().await?);
        assert_eq!(phase(&pool).await, expected);
    }
    assert_eq!(counts(&pool).await, (9, 5, 2));
    let missing: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM analytics.alert_evaluation_receipts WHERE disposition='missing'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(missing, 2);
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn historical_and_late_commits_are_receipted_without_fresh_notifications(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let base = setup(&pool, 0, 0).await?;
    let worker = PostgresAlertLifecycle::new(pool.clone());
    append(&pool, "old", base - 2000, Some("51")).await?;
    assert!(worker.advance_one().await?);
    assert_eq!(phase(&pool).await, "inactive");
    assert_eq!(counts(&pool).await, (1, 0, 0));
    append(&pool, "new", base + 2000, Some("51")).await?;
    assert!(worker.advance_one().await?);
    assert_eq!(phase(&pool).await, "firing");
    append(&pool, "late", base, Some("49")).await?;
    assert!(worker.advance_one().await?);
    append_live(
        &pool,
        &record("equal", base + 2000, Some("49"), "result", "c2"),
        "result",
    )
    .await?;
    assert!(worker.advance_one().await?);
    assert!(!worker.advance_one().await?);
    assert_eq!(phase(&pool).await, "firing");
    assert_eq!(counts(&pool).await, (4, 1, 1));
    let dispositions: Vec<String> = sqlx::query_scalar(
        "SELECT disposition FROM analytics.alert_evaluation_receipts ORDER BY evaluation_id",
    )
    .fetch_all(&pool)
    .await?;
    assert_eq!(
        dispositions,
        ["out_of_order", "out_of_order", "applied", "historical"]
    );
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_consumers_and_visible_order_do_not_duplicate_or_reorder_transitions(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let base = setup(&pool, 1000, 0).await?;
    append(&pool, "z-later", base + 1000, Some("51")).await?;
    append(&pool, "a-first", base, Some("51")).await?;
    let a = PostgresAlertLifecycle::new(pool.clone());
    let b = PostgresAlertLifecycle::new(pool.clone());
    let (x, y) = tokio::join!(a.advance_one(), b.advance_one());
    assert!(x? || y?);
    while a.advance_one().await? {}
    assert_eq!(counts(&pool).await, (2, 2, 1));
    let events: Vec<String> = sqlx::query_scalar(
        "SELECT evaluation_id FROM analytics.alert_transitions ORDER BY version",
    )
    .fetch_all(&pool)
    .await?;
    assert_eq!(events, ["a-first", "z-later"]);
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn failed_outbox_leaves_every_checkpoint_retryable(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let base = setup(&pool, 0, 0).await?;
    append(&pool, "fire", base, Some("51")).await?;
    sqlx::raw_sql("CREATE FUNCTION analytics.reject_test_intent() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected outbox failure'; END $$; CREATE TRIGGER reject_test_intent BEFORE INSERT ON analytics.alert_notification_intents FOR EACH ROW EXECUTE FUNCTION analytics.reject_test_intent();").execute(&pool).await?;
    let worker = PostgresAlertLifecycle::new(pool.clone());
    assert!(worker.advance_one().await.is_err());
    assert_eq!(phase(&pool).await, "inactive");
    assert_eq!(counts(&pool).await, (0, 0, 0));
    sqlx::query("DROP TRIGGER reject_test_intent ON analytics.alert_notification_intents")
        .execute(&pool)
        .await?;
    assert!(worker.advance_one().await?);
    assert_eq!(counts(&pool).await, (1, 1, 1));
    for statement in [
        "DELETE FROM analytics.alert_transitions",
        "DELETE FROM analytics.alert_notification_intents",
        "DELETE FROM analytics.alert_evaluation_receipts",
    ] {
        assert!(sqlx::query(statement).execute(&pool).await.is_err());
    }
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn only_active_tenant_and_exact_revision_release_scope_target_are_consumed(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let base = setup(&pool, 0, 0).await?;
    let worker = PostgresAlertLifecycle::new(pool.clone());
    PostgresCalculatorEvaluationStore::new(pool.clone())
        .append(&record("other", base, Some("51"), "other", "other-config"))
        .await?;
    assert!(!worker.advance_one().await?);
    sqlx::query(
        "SELECT analytics.change_alert_rule_activation('tenant-a','high',1,NULL,'operator-a')",
    )
    .execute(&pool)
    .await?;
    append(&pool, "fire", base, Some("51")).await?;
    assert!(!worker.advance_one().await?);
    sqlx::query("SELECT analytics.create_alert_rule_revision('tenant-a','high',2,'operator-a',$1)")
        .bind(definition(0, 0))
        .execute(&pool)
        .await?;
    assert!(!worker.advance_one().await?);
    sqlx::query(
        "SELECT analytics.change_alert_rule_activation('tenant-a','high',2,1,'operator-a')",
    )
    .execute(&pool)
    .await?;
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await?;
    assert!(!worker.advance_one().await?);
    sqlx::query("UPDATE metadata.tenants SET is_active=true WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await?;
    assert!(worker.advance_one().await?);
    let untouched: (String, i64) =
        sqlx::query_as("SELECT phase,version FROM analytics.alert_lifecycles WHERE revision=2")
            .fetch_one(&pool)
            .await?;
    assert_eq!(untouched, ("inactive".into(), 0));
    assert_eq!(counts(&pool).await.0, 1);
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn one_shot_replay_and_reactivation_do_not_satisfy_old_hold_candidates(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let base = setup(&pool, 1000, 0).await?;
    let worker = PostgresAlertLifecycle::new(pool.clone());
    PostgresCalculatorEvaluationStore::new(pool.clone())
        .append(&record(
            "replay",
            base,
            Some("51"),
            "result",
            "replay-config",
        ))
        .await?;
    assert!(worker.advance_one().await?);
    assert_eq!(counts(&pool).await, (1, 0, 0));
    append(&pool, "first", base, Some("51")).await?;
    assert!(worker.advance_one().await?);
    sqlx::query(
        "SELECT analytics.change_alert_rule_activation('tenant-a','high',1,NULL,'operator-a')",
    )
    .execute(&pool)
    .await?;
    append(&pool, "next", base + 1000, Some("51")).await?;
    assert!(!worker.advance_one().await?);
    sqlx::query(
        "SELECT analytics.change_alert_rule_activation('tenant-a','high',2,1,'operator-a')",
    )
    .execute(&pool)
    .await?;
    assert!(worker.advance_one().await?);
    assert_eq!(phase(&pool).await, "pending");
    assert_eq!(counts(&pool).await, (3, 1, 0));
    append(&pool, "third", base + 2000, Some("51")).await?;
    assert!(worker.advance_one().await?);
    assert_eq!(phase(&pool).await, "firing");
    assert_eq!(counts(&pool).await, (4, 2, 1));
    assert!(sqlx::query("INSERT INTO analytics.alert_notification_intents VALUES('tenant-a','high',1,1,clock_timestamp())").execute(&pool).await.is_err());
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn cancellation_at_the_outbox_boundary_rolls_back_before_restart(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let base = setup(&pool, 0, 0).await?;
    append(&pool, "fire", base, Some("51")).await?;
    sqlx::raw_sql("CREATE FUNCTION analytics.block_test_intent() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(74423); RETURN NEW; END $$; CREATE TRIGGER block_test_intent BEFORE INSERT ON analytics.alert_notification_intents FOR EACH ROW EXECUTE FUNCTION analytics.block_test_intent();").execute(&pool).await?;
    let mut barrier = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(74423)")
        .execute(&mut *barrier)
        .await?;
    let worker = PostgresAlertLifecycle::new(pool.clone());
    let task = tokio::spawn(async move { worker.advance_one().await });
    tokio::time::timeout(std::time::Duration::from_secs(5),async {
        loop {
            let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND objid=74423 AND NOT granted AND database=(SELECT oid FROM pg_database WHERE datname=current_database()))").fetch_one(&pool).await.unwrap();
            if blocked { break; }
            tokio::task::yield_now().await;
        }
    }).await?;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    barrier.rollback().await?;
    sqlx::query("DROP TRIGGER block_test_intent ON analytics.alert_notification_intents")
        .execute(&pool)
        .await?;
    assert_eq!(counts(&pool).await, (0, 0, 0));
    assert_eq!(phase(&pool).await, "inactive");
    assert!(
        PostgresAlertLifecycle::new(pool.clone())
            .advance_one()
            .await?
    );
    assert_eq!(counts(&pool).await, (1, 1, 1));
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn equal_rule_and_evaluation_ids_in_another_tenant_never_share_state(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let base = setup(&pool, 0, 0).await?;
    append(&pool, "shared", base, Some("51")).await?;
    sqlx::raw_sql("INSERT INTO identity.tenant_action_grants VALUES ('tenant-b','operator-a','alerts.manage');
      INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind,canonical_unit)
        SELECT 'tenant-b',property_id,version,display_name,value_kind,canonical_unit FROM metadata.property_definitions WHERE tenant_id='tenant-a' AND property_id IN ('electrical.voltage','mean.voltage');
      INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count) VALUES ('tenant-b','calculated',1,'Calculated',1);
      INSERT INTO metadata.asset_type_properties (tenant_id,type_id,type_version,property_id,property_version,property_ordinal) VALUES ('tenant-b','calculated',1,'mean.voltage',1,1);
      INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name) VALUES ('tenant-b','asset-a','calculated',1,'Scope'),('tenant-b','result','calculated',1,'Result');
      INSERT INTO analytics.calculator_definitions VALUES ('tenant-b','mean','1','window-mean-v1','electrical.voltage',1,'mean.voltage',1),('tenant-a','mean','2','window-mean-v1','electrical.voltage',1,'mean.voltage',1);").execute(&pool).await?;
    sqlx::query("SELECT analytics.create_alert_rule_revision('tenant-b','high',1,'operator-a',$1)")
        .bind(definition(0, 0))
        .execute(&pool)
        .await?;
    sqlx::query(
        "SELECT analytics.change_alert_rule_activation('tenant-b','high',0,1,'operator-a')",
    )
    .execute(&pool)
    .await?;
    for (tenant, id, patch) in [
        ("tenant-a", "wrong-scope", json!({"scope":"asset-b"})),
        (
            "tenant-a",
            "wrong-release",
            json!({"version":"2","configuration":"release-2"}),
        ),
        ("tenant-b", "shared", json!({})),
    ] {
        sqlx::query("INSERT INTO analytics.evaluations(tenant_id,evaluation_id,replay_key,key_digest,evaluated_at_ms,outcome,outputs,minimum_samples) SELECT $1,$2,replay_key||$3,sha256(convert_to((replay_key||$3)::text,'UTF8')),evaluated_at_ms,outcome,outputs,minimum_samples FROM analytics.evaluations WHERE tenant_id='tenant-a' AND evaluation_id='shared'")
            .bind(tenant).bind(id).bind(patch).execute(&pool).await?;
    }
    let worker = PostgresAlertLifecycle::new(pool.clone());
    while worker.advance_one().await? {}
    assert_eq!(counts(&pool).await, (2, 1, 1));
    let states: Vec<(String, String)> =
        sqlx::query_as("SELECT tenant_id,phase FROM analytics.alert_lifecycles ORDER BY tenant_id")
            .fetch_all(&pool)
            .await?;
    assert_eq!(
        states,
        [
            ("tenant-a".into(), "firing".into()),
            ("tenant-b".into(), "inactive".into())
        ]
    );
    assert!(sqlx::query("INSERT INTO analytics.alert_notification_intents VALUES('tenant-b','high',1,1,clock_timestamp())").execute(&pool).await.is_err());
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn checkpoint_creation_boundary_suppresses_a_predeployment_active_rule_backlog(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool, 0, 0).await?;
    // A rule could already have been selected before the lifecycle migration.
    sqlx::raw_sql("INSERT INTO analytics.alert_rule_activation_changes(tenant_id,rule_id,generation,revision,actor_id,recorded_at_ms) VALUES('tenant-a','high',2,1,'operator-a',0); UPDATE analytics.alert_rules SET generation=2,active_revision=1 WHERE tenant_id='tenant-a' AND rule_id='high';").execute(&pool).await?;
    append(&pool, "old-scheduled", 1000, Some("51")).await?;
    assert!(
        PostgresAlertLifecycle::new(pool.clone())
            .advance_one()
            .await?
    );
    assert_eq!(counts(&pool).await, (1, 0, 0));
    assert_eq!(phase(&pool).await, "inactive");
    let disposition: String =
        sqlx::query_scalar("SELECT disposition FROM analytics.alert_evaluation_receipts")
            .fetch_one(&pool)
            .await?;
    assert_eq!(disposition, "historical");
    assert!(
        sqlx::query("UPDATE analytics.alert_lifecycles SET live_from_ms=0")
            .execute(&pool)
            .await
            .is_err()
    );
    Ok(())
}

//! Exercise the real checked-schema command, durable effects and graceful stop.
use sqlx::{ConnectOptions, PgPool};
use std::{error::Error, process::Stdio, time::Duration};
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn analytics_worker_consumes_selected_rules_without_starting_delivery(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    sqlx::raw_sql(include_str!(
        "../../postgres/tests/property_history/topology.sql"
    ))
    .execute(&pool)
    .await?;
    sqlx::raw_sql("INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-a','alerts.manage');
      INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind,canonical_unit) VALUES ('tenant-a','mean.voltage',1,'Mean','decimal','volt');
      INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count) VALUES ('tenant-a','calculated',1,'Calculated',1);
      INSERT INTO metadata.asset_type_properties (tenant_id,type_id,type_version,property_id,property_version,property_ordinal) VALUES ('tenant-a','calculated',1,'mean.voltage',1,1);
      INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name) VALUES ('tenant-a','result','calculated',1,'Result');
      INSERT INTO analytics.calculator_definitions VALUES ('tenant-a','mean','1','window-mean-v1','electrical.voltage',1,'mean.voltage',1);
      SELECT analytics.create_alert_rule_revision('tenant-a','high',1,'operator-a','{\"kind\":\"numeric-threshold-v1\",\"calculator\":\"mean\",\"version\":\"1\",\"scope\":\"asset-a\",\"target\":\"result\",\"output_property\":\"mean.voltage\",\"output_version\":1,\"output_index\":0,\"comparison\":\"gt\",\"threshold\":\"50\",\"threshold_unit\":\"volt\",\"hold_ms\":0,\"resolution\":\"resolve_on_false\",\"resolve_after_ms\":0}');
      SELECT analytics.change_alert_rule_activation('tenant-a','high',0,1,'operator-a');
      INSERT INTO analytics.calculator_schedules(tenant_id,schedule_id,calculator_id,calculator_version,configuration_id,scope_asset_id,target_asset_id,first_start_ms,window_ms,lateness_ms,minimum_samples,next_start_ms)
        VALUES('tenant-a','s','mean','1','c','asset-a','result',0,1000,0,1,0);
      WITH evidence AS (SELECT jsonb_build_object('scope','asset-a','calculator','mean','version','1','configuration','c',
          'start',recorded_at_ms,'end',recorded_at_ms+1000,'finalize',recorded_at_ms+1000,'known',recorded_at_ms+1000,'inputs','[]'::jsonb) AS k
        FROM analytics.alert_rule_activation_changes WHERE tenant_id='tenant-a' AND rule_id='high' AND generation=1)
      INSERT INTO analytics.evaluations(tenant_id,evaluation_id,replay_key,key_digest,evaluated_at_ms,outcome,outputs,minimum_samples)
        SELECT 'tenant-a','e',k,sha256(convert_to(k::text,'UTF8')),(k->>'end')::bigint,'complete',
          '[{\"asset\":\"result\",\"property\":\"mean.voltage\",\"version\":1,\"kind\":\"decimal\",\"value\":\"51\",\"unit\":\"volt\"}]'::jsonb,1 FROM evidence;
      INSERT INTO analytics.calculator_work(tenant_id,evaluation_id,schedule_id,start_ms,request,generation,status)
        VALUES('tenant-a','e','s',0,'{\"target\":\"result\"}',1,'done');").execute(&pool).await?;
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_fleetiq-server"))
        .arg("analytics-worker")
        .env(
            "DATABASE_URL",
            pool.connect_options().to_url_lossy().as_str(),
        )
        .env_remove("FLEETIQ_CALCULATOR_SCHEDULE_FILE")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            assert!(
                child.try_wait()?.is_none(),
                "worker exited before consuming evidence"
            );
            let ready: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM analytics.alert_notification_intents)",
            )
            .fetch_one(&pool)
            .await?;
            if ready {
                break;
            }
            tokio::task::yield_now().await;
        }
        Ok::<(), Box<dyn Error>>(())
    })
    .await??;
    let pid = child.id().ok_or("worker already exited")?.to_string();
    assert!(
        tokio::process::Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .await?
            .success()
    );
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait()).await??;
    assert!(status.success());
    let state:(String,i64,i64)=sqlx::query_as("SELECT phase,(SELECT count(*) FROM analytics.alert_transitions),(SELECT count(*) FROM analytics.alert_notification_intents) FROM analytics.alert_lifecycles").fetch_one(&pool).await?;
    assert_eq!(state, ("firing".into(), 1, 1));
    Ok(())
}

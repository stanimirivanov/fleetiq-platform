use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use tower::ServiceExt;
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROOT: &str = "/api/v1/tenants/tenant-a/assets/asset-a/calculator-evaluations";
async fn setup(pool: &PgPool) -> Router {
    sqlx::raw_sql(include_str!(
        "../../../postgres/tests/property_history/topology.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::raw_sql("INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-a','metadata.read'),('tenant-b','operator-a','telemetry.read');
      INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind,canonical_unit) VALUES ('tenant-a','mean.voltage',1,'Mean','decimal','volt');
      INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count) VALUES ('tenant-a','calculated',1,'Calculated',1);
      INSERT INTO metadata.asset_type_properties (tenant_id,type_id,type_version,property_id,property_version,property_ordinal) VALUES ('tenant-a','calculated',1,'mean.voltage',1,1);
      INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name) VALUES ('tenant-a','result','calculated',1,'Result');
      INSERT INTO analytics.calculator_definitions VALUES ('tenant-a','mean','1','window-mean-v1','electrical.voltage',1,'mean.voltage',1);").execute(pool).await.unwrap();
    let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "operator")
        .unwrap();
    let auth = fleetiq_access::CallerAuthenticationService::new(
        crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
            config.bearer_identity().unwrap().clone(),
        ),
        fleetiq_postgres::PostgresActorIdentityResolver::new(pool.clone()),
    );
    router(pool.clone(), auth)
}
async fn insert(pool: &PgPool, id: &str, inputs: Vec<Value>, outputs: Vec<Value>, outcome: &str) {
    let key = json!({"scope":"asset-a","calculator":"mean","version":"1","configuration":id,"start":0,"end":3000,"finalize":3000,"known":3000,"inputs":inputs});
    sqlx::query("INSERT INTO analytics.evaluations(tenant_id,evaluation_id,replay_key,key_digest,evaluated_at_ms,outcome,outputs,minimum_samples) VALUES ('tenant-a',$1,$2,sha256(convert_to($2::jsonb::text,'UTF8')),3500,$3,$4,1)").bind(id).bind(key).bind(outcome).bind(json!(outputs)).execute(pool).await.unwrap();
}
fn output(value: &str) -> Value {
    json!({"asset":"result","property":"mean.voltage","version":1,"kind":"decimal","value":value,"unit":"volt"})
}
async fn get(app: &Router, suffix: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .uri(format!("{ROOT}{suffix}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    if status == StatusCode::OK {
        let name = if suffix.contains("/outputs") {
            "CalculatorOutputPage"
        } else if suffix.starts_with('?') {
            "CalculatorEvaluationPage"
        } else {
            "CalculatorEvaluationEvidence"
        };
        let spec: Value =
            serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json"))
                .unwrap();
        let schema =
            json!({"components":spec["components"],"$ref":format!("#/components/schemas/{name}")});
        assert!(
            jsonschema::validator_for(&schema).unwrap().is_valid(&body),
            "{name} failed schema validation"
        );
    }
    (status, body)
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn ties_exact_evidence_and_output_positions_preserve_identity(pool: PgPool) {
    let app = setup(&pool).await;
    for id in ["a", "b", "c"] {
        insert(
            &pool,
            id,
            vec![],
            vec![
                output("12345678901234567890.1234567890123456789"),
                output("2"),
            ],
            "complete",
        )
        .await;
    }
    assert_eq!(
        get(&app, "?start_at_ms=0&end_at_ms=3000").await.1["evaluations"],
        json!([])
    );
    assert_eq!(
        get(&app, "?start_at_ms=3000&end_at_ms=3001").await.1["evaluations"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let first = get(&app, "?start_at_ms=0&end_at_ms=4000&limit=2").await;
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(first.1["evaluations"][0]["evaluation_id"], "a");
    assert_eq!(first.1["evaluations"][1]["evaluation_id"], "b");
    let token = first.1["next_after"].as_str().unwrap();
    let next = get(
        &app,
        &format!("?start_at_ms=0&end_at_ms=4000&limit=2&after={token}"),
    )
    .await
    .1;
    assert_eq!(next["evaluations"].as_array().unwrap().len(), 1);
    assert_eq!(next["evaluations"][0]["evaluation_id"], "c");
    assert!(next["next_after"].is_null());
    let exact = get(&app, "/a").await.1;
    assert_eq!(exact["evaluation"], first.1["evaluations"][0]);
    assert_eq!(exact["inputs"], json!([]));
    let first_output = get(&app, "/a/outputs?limit=1").await.1;
    assert_eq!(
        first_output["outputs"][0]["value"],
        "12345678901234567890.1234567890123456789"
    );
    assert_eq!(first_output["outputs"][0]["output_index"], 0);
    let token = first_output["next_after"].as_str().unwrap();
    let next = get(&app, &format!("/a/outputs?limit=1&after={token}"))
        .await
        .1;
    assert_eq!(next["outputs"][0]["output_index"], 1);
    assert_eq!(next["outputs"][0]["evaluation_id"], "a");
    assert!(next["next_after"].is_null());
    assert_eq!(
        get(&app, &format!("/b/outputs?after={token}")).await.0,
        StatusCode::BAD_REQUEST
    );
    insert(&pool, "empty", vec![], vec![], "insufficient").await;
    assert_eq!(
        get(&app, "/empty/outputs").await.1,
        json!({"outputs":[],"next_after":null})
    );
    assert_eq!(get(&app, "/missing").await.0, StatusCode::NOT_FOUND);
    assert_eq!(get(&app, "/missing/outputs").await.0, StatusCode::NOT_FOUND);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn scope_bound_cursors_and_authorization_precede_discovery(pool: PgPool) {
    let app = setup(&pool).await;
    for id in ["a", "b"] {
        insert(&pool, id, vec![], vec![], "insufficient").await;
    }
    let first = get(&app, "?start_at_ms=0&end_at_ms=4000&limit=1").await.1;
    let token = first["next_after"].as_str().unwrap();
    assert_eq!(
        get(
            &app,
            &format!("?start_at_ms=1&end_at_ms=4000&after={token}")
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    for resource in [
        ROOT.replace("asset-a", "asset-b"),
        ROOT.replace("tenant-a", "tenant-b"),
    ] {
        let request = Request::builder()
            .uri(format!(
                "{resource}?start_at_ms=0&end_at_ms=4000&after={token}"
            ))
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            StatusCode::BAD_REQUEST
        );
    }
    for suffix in [
        "?start_at_ms=0&end_at_ms=0",
        "?start_at_ms=0&end_at_ms=604800001",
        "?start_at_ms=-9223372036854775808&end_at_ms=9223372036854775807",
        "?start_at_ms=00&end_at_ms=1",
        "?start_at_ms=0&end_at_ms=4000&limit=101",
        "?start_at_ms=0&end_at_ms=4000&after=v1.00",
        "/a?raw=true",
        "/a/outputs?limit=21",
        "/a/outputs?limit=0",
    ] {
        assert_eq!(
            get(&app, suffix).await.0,
            StatusCode::BAD_REQUEST,
            "{suffix}"
        );
    }
    let request = Request::builder()
        .uri(format!("{ROOT}/a"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND action='telemetry.read'").execute(&pool).await.unwrap();
    assert_eq!(get(&app, "/a").await.0, StatusCode::FORBIDDEN);
    assert_eq!(get(&app, "?malformed=true").await.0, StatusCode::FORBIDDEN);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn maximum_manifest_and_outputs_remain_separately_bounded(pool: PgPool) {
    let app = setup(&pool).await;
    sqlx::raw_sql("INSERT INTO ingestion.accepted_messages (tenant_id,message_id,device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload) SELECT tenant_id,'many',device_asset_id,protocol_package_id,protocol_package_version,2500,payload FROM ingestion.accepted_messages WHERE message_id='message-1';
      INSERT INTO ingestion.decoded_message_results VALUES ('tenant-a','many','vendor.protocol','1',1022);
      INSERT INTO ingestion.decoded_observations (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,endpoint_id,signal_id,device_recorded_at_ms,value_kind,value_integer,source_unit,quality) SELECT o.tenant_id,'many',o.protocol_package_id,o.protocol_package_version,n,o.endpoint_id,o.signal_id,1500,o.value_kind,1,o.source_unit,o.quality FROM ingestion.decoded_observations o CROSS JOIN generate_series(0,1021) n WHERE message_id='message-1' AND output_index=0;
      INSERT INTO history.asset_property_changes SELECT 'tenant-a','many','vendor.protocol','1',n,'binding-1',1,'device_recorded',1500,1500,150 FROM generate_series(0,1021) n;").execute(&pool).await.unwrap();
    let inputs:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_array(analytics.semantic_evidence_id(tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis),event_at_ms) FROM history.asset_property_changes ORDER BY event_at_ms,analytics.semantic_evidence_id(tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis) COLLATE \"C\"").fetch_all(&pool).await.unwrap();
    assert_eq!(inputs.len(), 1024);
    let expected = inputs[0][0].clone();
    insert(
        &pool,
        "maximum",
        inputs,
        vec![output(&"9".repeat(4096)); 1024],
        "complete",
    )
    .await;
    let exact = get(&app, "/maximum").await.1;
    assert_eq!(exact["inputs"].as_array().unwrap().len(), 1024);
    assert_eq!(exact["inputs"][0]["evidence_id"], expected);
    assert_eq!(exact["evaluation"]["output_count"], 1024);
    assert!(exact.get("outputs").is_none());
    let outputs = get(&app, "/maximum/outputs").await.1;
    assert_eq!(outputs["outputs"].as_array().unwrap().len(), 20);
    assert_eq!(outputs["outputs"][0]["value"].as_str().unwrap().len(), 4096);
    assert!(outputs["next_after"].is_string());
    sqlx::query("WITH generated AS (SELECT 'page-'||n AS id,e.replay_key||jsonb_build_object('configuration','page-'||n,'inputs','[]'::jsonb) AS key FROM analytics.evaluations e CROSS JOIN generate_series(0,100) n WHERE evaluation_id='maximum') INSERT INTO analytics.evaluations(tenant_id,evaluation_id,replay_key,key_digest,evaluated_at_ms,outcome,outputs,minimum_samples) SELECT 'tenant-a',id,key,sha256(convert_to(key::text,'UTF8')),3500,'insufficient','[]'::jsonb,1 FROM generated").execute(&pool).await.unwrap();
    let summaries = get(&app, "?start_at_ms=0&end_at_ms=4000&limit=100").await.1;
    assert_eq!(summaries["evaluations"].as_array().unwrap().len(), 100);
    assert!(summaries["evaluations"][0].get("inputs").is_none());
    let token = summaries["next_after"].as_str().unwrap();
    assert_eq!(
        get(
            &app,
            &format!("?start_at_ms=0&end_at_ms=4000&limit=100&after={token}")
        )
        .await
        .1["evaluations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

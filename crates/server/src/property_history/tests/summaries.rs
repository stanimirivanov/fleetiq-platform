//! Published summaries remain separate, exact and pinned across continuations.
use super::*;
use fleetiq_domain::{
    AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, SignalTarget, TenantId,
};
use fleetiq_history::downsampling::{NumericRequest, NumericSpec};
fn selection() -> NumericRequest {
    NumericRequest::new(
        TenantId::new("tenant-a").unwrap(),
        SignalTarget::new(
            AssetId::new("asset-a").unwrap(),
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("electrical.voltage").unwrap(),
                DefinitionVersion::new(1).unwrap(),
            ),
        ),
        NumericSpec::new(0, 3000, 1000).unwrap(),
        None,
    )
}
async fn published(pool: &PgPool) -> Router {
    let app = setup(pool).await;
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES('tenant-a',1,'operator-a',7,30,3,8388608)").execute(pool).await.unwrap();
    crate::produce_numeric_summary_once(pool.clone(), &selection())
        .await
        .unwrap();
    app
}
fn url() -> String {
    format!("{ROOT}/summaries?start_at_ms=0&end_at_ms=3000&resolution_ms=1000&limit=1")
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn summary_pages_keep_exact_numbers_coverage_and_original_revision(pool: PgPool) {
    let app = published(&pool).await;
    let url = url();
    let (status, first) = response(&app, &url).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["representation"], "numeric_summary");
    assert_eq!(first["resolution_ms"], "1000");
    assert_eq!(first["revision"], "1");
    assert_eq!(first["coverage"]["scope"], "hot_snapshot");
    assert_eq!(first["buckets"][0]["count"], 0);
    let cursor = first["next_after"].as_str().unwrap();
    sqlx::query("INSERT INTO history.asset_property_changes VALUES('tenant-a','message-1','vendor.protocol','1',1,'binding-1',1,'device_recorded',500,500,150)").execute(&pool).await.unwrap();
    crate::produce_numeric_summary_once(pool.clone(), &selection())
        .await
        .unwrap();
    let (status, next) = response(&app, &format!("{url}&after={cursor}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(next["revision"], "1");
    assert_eq!(next["buckets"][0]["sum"], "9223372036854775807");
    assert_eq!(next["buckets"][0]["min"], "9223372036854775807");
    let (_, fresh) = response(&app, &url).await;
    assert_eq!(fresh["revision"], "2");
    assert_eq!(fresh["buckets"][0]["count"], 1);
    let contract: Value = serde_json::from_str(include_str!(
        "../../../../../contracts/http/dist/openapi.json"
    ))
    .unwrap();
    let schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","components":{"schemas":contract["components"]["schemas"]},"$ref":"#/components/schemas/NumericSummaryPage"});
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&next));
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn summary_bounds_cursors_and_missing_scope_are_explicit(pool: PgPool) {
    let app = published(&pool).await;
    let url = url();
    let (_, page) = response(&app, &url).await;
    let token = page["next_after"].as_str().unwrap();
    for bad in [
        format!("{url}&after=broken"),
        format!("{url}&unknown=yes"),
        format!("{url}&manifest_id=1"),
        format!("{url}&manifest_id=0&restore_generation=1"),
        format!("{url}&after={token}&manifest_id=1&restore_generation=1"),
        url.replace("end_at_ms=3000", "end_at_ms=604801000"),
        url.replace("limit=1", "limit=0"),
        url.replace("limit=1", "limit=65"),
        url.replace("end_at_ms=3000", "end_at_ms=65000"),
        url.replace("start_at_ms=0", "start_at_ms=1"),
        url.replace("resolution_ms=1000", "resolution_ms=0"),
        url.replace("resolution_ms=1000", "resolution_ms=01000"),
        format!(
            "{}&after={token}",
            url.replace("end_at_ms=3000", "end_at_ms=4000")
        ),
        format!("{}&after={token}", url.replace("asset-a", "asset-b")),
    ] {
        assert_eq!(
            response(&app, &bad).await.0,
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
    assert_eq!(
        response(&app, &url.replace("asset-a", "asset-b")).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        response(
            &app,
            &url.replace("resolution_ms=1000", "resolution_ms=1500")
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn summary_authorization_is_current_before_validation_and_every_page(pool: PgPool) {
    let app = published(&pool).await;
    let url = url();
    let (_, page) = response(&app, &url).await;
    let cursor = page["next_after"].as_str().unwrap();
    assert_eq!(
        app.clone()
            .oneshot(request(&format!("{url}&after=bad"), false))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        response(&app, &url.replace("tenant-a", "tenant-b")).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND action='telemetry.read'").execute(&pool).await.unwrap();
    for path in [
        url.clone(),
        format!("{url}&after={cursor}"),
        format!("{url}&after=bad"),
    ] {
        assert_eq!(response(&app, &path).await.0, StatusCode::FORBIDDEN);
    }
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn summary_decimal_precision_and_excluded_quality_are_preserved(pool: PgPool) {
    let app = published(&pool).await;
    sqlx::raw_sql("INSERT INTO ingestion.accepted_messages(tenant_id,message_id,device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload) SELECT tenant_id,'decimal-message',device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload FROM ingestion.accepted_messages;
 INSERT INTO ingestion.decoded_message_results VALUES ('tenant-a','decimal-message','vendor.protocol','1',2);
 INSERT INTO ingestion.decoded_observations(tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,endpoint_id,signal_id,device_recorded_at_ms,value_kind,value_decimal,quality)
 VALUES ('tenant-a','decimal-message','vendor.protocol','1',0,'can-primary','exact',1000,'decimal',9007199254740993.00000000000000000001,'good'),('tenant-a','decimal-message','vendor.protocol','1',1,'can-primary','exact',2000,'decimal',0.00000000000000000002,'uncertain');
 INSERT INTO history.asset_property_changes VALUES ('tenant-a','decimal-message','vendor.protocol','1',0,'binding-decimal',1,'device_recorded',1000,1000,150),('tenant-a','decimal-message','vendor.protocol','1',1,'binding-decimal',1,'device_recorded',2000,1000,150)").execute(&pool).await.unwrap();
    let req = NumericRequest::new(
        TenantId::new("tenant-a").unwrap(),
        SignalTarget::new(
            AssetId::new("asset-a").unwrap(),
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("exact.decimal").unwrap(),
                DefinitionVersion::new(1).unwrap(),
            ),
        ),
        selection().spec(),
        None,
    );
    crate::produce_numeric_summary_once(pool.clone(), &req)
        .await
        .unwrap();
    let (status, body) = response(
        &app,
        &url()
            .replace("electrical.voltage", "exact.decimal")
            .replace("limit=1", "limit=64"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["kind"], "decimal");
    assert!(body["unit"].is_null());
    assert_eq!(
        body["buckets"][1]["sum"],
        "9007199254740993.00000000000000000001"
    );
    assert_eq!(body["buckets"][2]["excluded"], 1);
    assert_eq!(body["buckets"][2]["count"], 0);
    assert!(body["buckets"][2]["min"].is_null());
    assert!(body["next_after"].is_null());
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn summary_budget_denial_and_inactive_tenant_do_not_serve_evidence(pool: PgPool) {
    let app = published(&pool).await;
    let (_, page) = response(&app, &url()).await;
    let mut cursor: Value =
        crate::cursor_codec::decode(page["next_after"].as_str().unwrap()).unwrap();
    cursor["offset"] = json!(3);
    let invalid_position = crate::cursor_codec::encode(&cursor).unwrap();
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES('tenant-a',2,'operator-a',7,30,1,1)").execute(&pool).await.unwrap();
    assert_eq!(
        response(&app, &url()).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        response(&app, &format!("{}&after={invalid_position}", url()))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(response(&app, &url()).await.0, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn published_selected_coverage_and_continuation_survive_staging_expiry(pool: PgPool) {
    use fleetiq_history::restore::{RestoreClaim, RestoreRepository};
    let app = setup(&pool).await;
    let (manifest, segment) = super::restored::publish(&pool).await;
    let tenant = TenantId::new("tenant-a").unwrap();
    let restores = fleetiq_postgres::PostgresRestore::new(pool.clone());
    let ticket = restores.request(&tenant, manifest, 0).await.unwrap();
    let RestoreClaim::Claimed(lease) = restores.claim(&tenant, ticket).await.unwrap() else {
        panic!("restore")
    };
    assert!(restores.complete(&lease, &segment).await.unwrap());
    let base = selection();
    let req = NumericRequest::new(tenant, base.target().clone(), base.spec(), Some(ticket));
    crate::produce_numeric_summary_once(pool.clone(), &req)
        .await
        .unwrap();
    let url = format!(
        "{}&manifest_id={manifest}&restore_generation={}",
        url(),
        ticket.generation()
    );
    let (status, first) = response(&app, &url).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        first["coverage"]["scope"],
        "hot_and_selected_manifest_snapshot"
    );
    assert_eq!(first["coverage"]["manifest_id"], manifest.to_string());
    let cursor = first["next_after"].as_str().unwrap();
    sqlx::query("UPDATE history.restore_jobs SET expires_at=clock_timestamp()-interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let (status, next) = response(&app, &format!("{url}&after={cursor}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(next["coverage"], first["coverage"]);
    assert_eq!(next["revision"], first["revision"]);
    assert_eq!(next["buckets"][0]["count"], 1);
}

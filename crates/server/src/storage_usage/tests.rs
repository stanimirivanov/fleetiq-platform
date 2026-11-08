use crate::alerts::tests::setup;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;
async fn read(app: &Router, tenant: &str, token: bool) -> (StatusCode, Value) {
    read_at(app, tenant, token, "").await
}
async fn read_at(app: &Router, tenant: &str, token: bool, suffix: &str) -> (StatusCode, Value) {
    let mut request = Request::get(format!("/api/v1/tenants/{tenant}/storage-usage{suffix}"));
    if token {
        request = request.header(
            "authorization",
            "Bearer aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let status = response.status();
    let body =
        serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
    (status, body)
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn storage_usage_reconciles_rows_and_requires_current_independent_authority(pool: PgPool) {
    let app = setup(&pool).await;
    assert_eq!(
        read(&app, "tenant-a", false).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(read(&app, "tenant-a", true).await.0, StatusCode::FORBIDDEN);
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-a','operations.read')").execute(&pool).await.unwrap();
    let (status, body) = read(&app, "tenant-a", true).await;
    assert_eq!(status, StatusCode::OK);
    let spec: Value =
        serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json")).unwrap();
    assert_eq!(
        spec["paths"]["/api/v1/tenants/{tenant_id}/storage-usage"]["get"]["security"],
        serde_json::json!([{"bearerAuth":[]},{"cookieAuth":[]}])
    );
    let schema = serde_json::json!({"components":spec["components"],"$ref":"#/components/schemas/StorageUsage"});
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&body));
    assert_eq!(body["physical_allocation"], "not_measured");
    assert_eq!(body["archive"], "not_measured");
    let payloads: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT payload FROM ingestion.accepted_messages WHERE tenant_id='tenant-a'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(body["classes"][0]["records"], payloads.len().to_string());
    assert_eq!(
        body["classes"][0]["bytes"],
        payloads.iter().map(Vec::len).sum::<usize>().to_string()
    );
    for (index, sql) in [
        (
            1,
            "SELECT row_to_json(r)::text FROM ingestion.decoded_observations r WHERE tenant_id='tenant-a'",
        ),
        (
            2,
            "SELECT row_to_json(r)::text FROM history.asset_property_changes r WHERE tenant_id='tenant-a'",
        ),
    ] {
        let rows: Vec<String> = sqlx::query_scalar(sql).fetch_all(&pool).await.unwrap();
        assert_eq!(body["classes"][index]["records"], rows.len().to_string());
        assert_eq!(
            body["classes"][index]["bytes"],
            rows.iter().map(String::len).sum::<usize>().to_string()
        );
    }
    assert_eq!(read(&app, "tenant-b", true).await.0, StatusCode::FORBIDDEN);
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES ('tenant-b','operator-a','operations.read')").execute(&pool).await.unwrap();
    let (status, empty) = read(&app, "tenant-b", true).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        empty["classes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["records"] == "0" && m["bytes"] == "0")
    );
    let mut lock = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE ingestion.decoded_observations IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock)
        .await
        .unwrap();
    assert_eq!(
        read(&app, "tenant-a", true).await.0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    lock.rollback().await.unwrap();
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND action='operations.read'").execute(&pool).await.unwrap();
    assert_eq!(read(&app, "tenant-a", true).await.0, StatusCode::FORBIDDEN);
}

async fn lifecycle_fixture(pool: &PgPool) -> (i64, Vec<i64>) {
    use fleetiq_domain::{
        AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, SignalTarget,
        TenantId,
    };
    use fleetiq_history::{
        archive_work::{ArchiveClaim, ArchiveObject, ArchiveWorkRepository},
        exports::{ExportKey, ExportRequest, ExportTick},
    };
    use fleetiq_postgres::{PostgresArchiveWork, PostgresExports};
    use sha2::{Digest, Sha256};

    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,8,33554432)").execute(pool).await.unwrap();
    let tenant = TenantId::new("tenant-a").unwrap();
    let archive = PostgresArchiveWork::new(pool.clone());
    let ArchiveClaim::Claimed(lease) = archive.claim(&tenant).await.unwrap() else {
        panic!("archive claim");
    };
    assert!(
        lease.changes.len() > 1,
        "manifest counts must not multiply by members"
    );
    let bytes = fleetiq_archive::encode(&tenant, &lease.changes).unwrap();
    let len = i64::try_from(bytes.len()).unwrap();
    let object = ArchiveObject::new(Sha256::digest(&bytes).into(), len).unwrap();
    for _ in 0..2 {
        assert!(archive.complete(&lease, object).await.unwrap());
    }
    sqlx::query("INSERT INTO history.archive_jobs (tenant_id) VALUES ('tenant-a'),('tenant-b')")
        .execute(pool)
        .await
        .unwrap();

    let request = ExportRequest::new(
        tenant.clone(),
        SignalTarget::new(
            AssetId::new("asset-a").unwrap(),
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("electrical.voltage").unwrap(),
                DefinitionVersion::new(1).unwrap(),
            ),
        ),
        0,
        3000,
        None,
    )
    .unwrap();
    let exports = PostgresExports::new(pool.clone());
    let mut sizes = Vec::new();
    for name in ["downloadable", "pending", "expired", "cancelled"] {
        let key = ExportKey::new(name).unwrap();
        let ticket = crate::request_export(pool.clone(), &key, &request)
            .await
            .unwrap();
        if matches!(name, "downloadable" | "expired") {
            assert_eq!(
                crate::export_once(pool.clone(), &tenant, ticket)
                    .await
                    .unwrap(),
                ExportTick::Ready
            );
        }
        sizes.push(
            exports
                .describe(&tenant, ticket)
                .await
                .unwrap()
                .unwrap()
                .file_bytes,
        );
        if name == "cancelled" {
            exports.cancel(&tenant, ticket).await.unwrap();
        }
    }
    sqlx::query("UPDATE history.export_jobs SET expires_at=clock_timestamp() WHERE tenant_id='tenant-a' AND request_key='expired'").execute(pool).await.unwrap();
    (len, sizes)
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn lifecycle_storage_reconciles_committed_and_retained_artifacts(pool: PgPool) {
    let app = setup(&pool).await;
    let read = |tenant, auth| read_at(&app, tenant, auth, "/lifecycle");
    assert_eq!(read("tenant-a", false).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(read("tenant-a", true).await.0, StatusCode::FORBIDDEN);
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-a','operations.read')").execute(&pool).await.unwrap();
    let (archive_size, sizes) = lifecycle_fixture(&pool).await;
    let (status, body) = read("tenant-a", true).await;
    assert_eq!(status, StatusCode::OK);
    let spec: Value =
        serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json")).unwrap();
    let schema = serde_json::json!({"components":spec["components"],"$ref":"#/components/schemas/LifecycleStorageUsage"});
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&body));
    assert!(
        body["observed_at_ms"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .unwrap()
            > 0
    );
    assert_eq!(
        body["archive"],
        serde_json::json!({"records":"1","bytes":archive_size.to_string()})
    );
    for (kind, size) in ["downloadable", "pending", "expired_retained"]
        .into_iter()
        .zip(sizes)
    {
        assert_eq!(
            body["exports"][kind],
            serde_json::json!({"records":"1","bytes":size.to_string()})
        );
    }
    assert_eq!(read("tenant-b", true).await.0, StatusCode::FORBIDDEN);
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES ('tenant-b','operator-a','operations.read')").execute(&pool).await.unwrap();
    let (status, empty) = read("tenant-b", true).await;
    assert_eq!(status, StatusCode::OK);
    let zero = serde_json::json!({"records":"0","bytes":"0"});
    assert_eq!(empty["archive"], zero);
    for kind in ["downloadable", "pending", "expired_retained"] {
        assert_eq!(empty["exports"][kind], zero);
    }
    fleetiq_postgres::PostgresExports::new(pool.clone())
        .expire(&fleetiq_domain::TenantId::new("tenant-a").unwrap())
        .await
        .unwrap();
    assert_eq!(
        read("tenant-a", true).await.1["exports"]["expired_retained"],
        zero
    );
    let mut lock = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE history.export_jobs IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock)
        .await
        .unwrap();
    assert_eq!(
        read("tenant-a", true).await.0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    lock.rollback().await.unwrap();
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND action='operations.read'").execute(&pool).await.unwrap();
    assert_eq!(read("tenant-a", true).await.0, StatusCode::FORBIDDEN);
}

#[test]
fn lifecycle_totals_serialize_large_integers_exactly() {
    let measure = fleetiq_history::storage_usage::StorageMeasure::from_totals(
        "9007199254740993",
        "18446744073709551616",
    )
    .unwrap();
    assert_eq!(
        super::totals(&measure),
        serde_json::json!({
            "records": "9007199254740993", "bytes": "18446744073709551616",
        })
    );
}

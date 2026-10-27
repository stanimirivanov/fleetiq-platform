//! HTTP plus real PostgreSQL tests for policy, contract and immutable evidence.
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use fleetiq_access::CallerAuthenticationService;
use fleetiq_postgres::PostgresActorIdentityResolver;
use serde_json::{Value, json};
use tower::ServiceExt;
const ROOT: &str =
    "/api/v1/tenants/tenant-a/assets/asset-a/properties/electrical.voltage/versions/1";
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
async fn setup(pool: &PgPool) -> Router {
    // Shared synthetic SQL is the adapter's lineage-valid integration fixture.
    sqlx::raw_sql(include_str!(
        "../../../postgres/tests/property_history/topology.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "operator")
        .unwrap();
    router(
        pool.clone(),
        CallerAuthenticationService::new(
            crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
                config.bearer_identity().unwrap().clone(),
            ),
            PostgresActorIdentityResolver::new(pool.clone()),
        ),
    )
}
fn request(path: &str, credential: bool) -> Request<Body> {
    let mut request = Request::get(path);
    if credential {
        request = request.header("authorization", format!("Bearer {TOKEN}"));
    }
    request.body(Body::empty()).unwrap()
}
async fn response(app: &Router, path: &str) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request(path, true)).await.unwrap();
    let status = response.status();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    assert!(
        !std::str::from_utf8(&bytes)
            .unwrap()
            .contains("private-wire-sentinel")
    );
    (status, serde_json::from_slice(&bytes).unwrap())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn history_and_provenance_match_contract_and_keep_large_integer_precision(pool: PgPool) {
    let app = setup(&pool).await;
    let url = format!("{ROOT}/history?start_at_ms=0&end_at_ms=3000&limit=1");
    let (status, page) = response(&app, &url).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        page["changes"][0]["value"],
        json!({"kind":"integer","value":"9223372036854775807"})
    );
    let contract: Value =
        serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json")).unwrap();
    for (schema, body) in [
        ("PropertyHistoryPage", page.clone()),
        ("PropertyHistoryChange", page["changes"][0].clone()),
    ] {
        let schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","components":{"schemas":contract["components"]["schemas"]},"$ref":format!("#/components/schemas/{schema}")});
        assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&body));
    }
    let token = page["changes"][0]["interpretation"].as_str().unwrap();
    let (status, exact) = response(&app, &format!("{ROOT}/interpretations/{token}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(exact, page["changes"][0]);
    let cursor = page["next_after"].as_str().unwrap();
    let (_, next) = response(&app, &format!("{url}&after={cursor}")).await;
    assert_eq!(next["changes"][0]["event_at_ms"], "2000");
    assert!(next["next_after"].is_null());
    assert_eq!(
        response(
            &app,
            &format!("{ROOT}/history?start_at_ms=1&end_at_ms=3000&after={cursor}")
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        response(
            &app,
            &format!(
                "{}/interpretations/{token}",
                ROOT.replace("asset-a", "asset-b")
            )
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        response(
            &app,
            &format!(
                "{}/interpretations/{token}",
                ROOT.replace("versions/1", "versions/2")
            )
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    // All telemetry reads reauthorize; possessing a cursor or provenance key
    // never survives revocation, tenant suspension or actor suspension.
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='telemetry.read'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(response(&app, &url).await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        response(&app, &format!("{ROOT}/interpretations/{token}"))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn authorization_precedes_inputs_and_failures_never_become_empty_history(pool: PgPool) {
    let app = setup(&pool).await;
    let invalid = format!("{ROOT}/history?start_at_ms=bad");
    assert_eq!(
        app.clone()
            .oneshot(request(&invalid, false))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let url = format!("{ROOT}/history?start_at_ms=0&end_at_ms=3000");
    assert_eq!(
        response(&app, &url.replace("tenant-a", "tenant-b")).await.0,
        StatusCode::FORBIDDEN
    );
    for suffix in [
        "?start_at_ms=0&end_at_ms=0",
        "?start_at_ms=0&end_at_ms=604800001",
        "?start_at_ms=0&end_at_ms=3000&limit=101",
        "?start_at_ms=0&end_at_ms=3000&limit=0",
        "?start_at_ms=0&end_at_ms=3000&after=v2.00",
        "?start_at_ms=01&end_at_ms=3000",
        "?start_at_ms=-9223372036854775808&end_at_ms=9223372036854775807",
        "?start_at_ms=0&end_at_ms=3000&unknown=true",
    ] {
        assert_eq!(
            response(&app, &format!("{ROOT}/history{suffix}")).await.0,
            StatusCode::BAD_REQUEST,
            "{suffix}"
        );
    }
    let (status, body) = response(&app, &url.replace("asset-a", "missing")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"changes":[],"next_after":null}));
    let (status, body) = response(&app, &format!("{ROOT}/interpretations/v1.00")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["title"], "Invalid request");
    sqlx::query("ALTER TABLE history.asset_property_changes RENAME TO unavailable_history")
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = response(&app, &url).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["title"], "Internal server error");
    assert!(!body.to_string().contains("asset_property_changes"));
}

#[test]
fn cursor_codec_rejects_wrong_scope_and_bounded_malformed_inputs() {
    use fleetiq_domain::{
        AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, SignalTarget,
        TenantId, UnixTimeMillis,
    };
    use fleetiq_history::{PropertyHistoryCursor, PropertyHistoryQuery};
    use fleetiq_semantics::{AssetPropertyChangeId, EventTimeBasis};
    use fleetiq_telemetry::{
        AcceptedMessageId, DecodedObservationId, ProtocolPackageId, ProtocolPackageRef,
        ProtocolPackageVersion,
    };
    let name = "🧭".repeat(128);
    let source = DecodedObservationId::new(
        AcceptedMessageId::new(&name).unwrap(),
        ProtocolPackageRef::new(
            ProtocolPackageId::new(&name).unwrap(),
            ProtocolPackageVersion::new(&name).unwrap(),
        ),
        u32::MAX,
    );
    let id = AssetPropertyChangeId::from_parts(
        source,
        fleetiq_domain::SignalBindingId::new(&name).unwrap(),
        fleetiq_domain::RecordRevision::new(u64::MAX).unwrap(),
        EventTimeBasis::ReceivedAt,
    );
    let tenant = TenantId::new(&name).unwrap();
    let target = SignalTarget::new(
        AssetId::new(&name).unwrap(),
        PropertyDefinitionRef::new(
            PropertyDefinitionId::new(&name).unwrap(),
            DefinitionVersion::new(u32::MAX).unwrap(),
        ),
    );
    let query = PropertyHistoryQuery::new(
        target.clone(),
        UnixTimeMillis::new(0),
        UnixTimeMillis::new(1),
        1,
        None,
    )
    .unwrap();
    let key = PropertyHistoryCursor::new(UnixTimeMillis::new(0), id.clone());
    let token = cursor::page_token(&tenant, &query, &key).unwrap();
    assert!(token.len() <= 8192);
    assert_eq!(cursor::page_key(&token, &tenant, &target, 0, 1), Some(key));
    assert!(cursor::page_key(&token, &TenantId::new("other").unwrap(), &target, 0, 1).is_none());
    let exact = cursor::encode(&cursor::Interpretation::from_id(&id)).unwrap();
    assert_eq!(
        cursor::decode::<cursor::Interpretation>(&exact)
            .unwrap()
            .id(),
        Some(id)
    );
    for token in [
        "v2.00".to_owned(),
        "v1.0".to_owned(),
        "v1.GG".to_owned(),
        "v1.00".to_owned(),
        "v1.éé".to_owned(),
        "v1.".to_owned() + &"0".repeat(8192),
    ] {
        assert!(cursor::decode::<cursor::Interpretation>(&token).is_none());
    }
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn suspended_tenant_or_actor_cannot_read_with_an_existing_telemetry_grant(pool: PgPool) {
    let app = setup(&pool).await;
    let url = format!("{ROOT}/history?start_at_ms=0&end_at_ms=3000");
    assert_eq!(response(&app, &url).await.0, StatusCode::OK);
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(response(&app, &url).await.0, StatusCode::FORBIDDEN);
    sqlx::query("UPDATE metadata.tenants SET is_active=true WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE identity.actors SET is_active=false WHERE actor_id='operator-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(response(&app, &url).await.0, StatusCode::UNAUTHORIZED);
}

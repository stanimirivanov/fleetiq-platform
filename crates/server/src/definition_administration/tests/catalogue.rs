//! Stable keyset traversal uses both identity and numeric version.
use super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn catalogue_and_version_pages_preserve_order_scope_and_current_values(pool: PgPool) {
    let app = setup(&pool).await;
    for (resource, payload) in definitions() {
        for version in [10, 1, 2] {
            let mut value = payload.clone();
            value["version"] = json!(version);
            publish(&app, resource, &value).await;
        }
        let path = format!("{ROOT}/{resource}");
        let all = send(&app, "GET", &path, &Value::Null, StatusCode::OK).await;
        crate::contract_test_support::assert_published_example(
            &serde_json::to_vec(&all).unwrap(),
            &format!("/api/v1/tenants/{{tenant_id}}/{resource}"),
        );
        let first = send(
            &app,
            "GET",
            &format!("{path}?limit=2"),
            &Value::Null,
            StatusCode::OK,
        )
        .await;
        assert_eq!(
            first["definitions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["version"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [1, 2]
        );
        let id = payload["id"].as_str().unwrap();
        assert_eq!(first["next_after"], json!({"id":id,"version":2}));
        let last = send(
            &app,
            "GET",
            &format!("{path}?limit=2&after_id={id}&after_version=2"),
            &Value::Null,
            StatusCode::OK,
        )
        .await;
        assert_eq!(last["definitions"][0]["version"], 10);
        assert_eq!(last["next_after"], Value::Null);
        let versions = send(
            &app,
            "GET",
            &format!("{path}/{id}/versions?limit=2"),
            &Value::Null,
            StatusCode::OK,
        )
        .await;
        assert_eq!(versions, first);
        let versions_last = send(
            &app,
            "GET",
            &format!("{path}/{id}/versions?after_id={id}&after_version=2"),
            &Value::Null,
            StatusCode::OK,
        )
        .await;
        assert_eq!(versions_last, last);
        let missing = send(
            &app,
            "GET",
            &format!("{path}/missing/versions"),
            &Value::Null,
            StatusCode::OK,
        )
        .await;
        assert_eq!(missing, json!({"definitions":[],"next_after":null}));
        let beyond = send(
            &app,
            "GET",
            &format!("{path}?after_id=zzz&after_version=1"),
            &Value::Null,
            StatusCode::OK,
        )
        .await;
        assert_eq!(beyond, missing);
        let mut next_identity = payload.clone();
        next_identity["id"] = json!("z.next");
        publish(&app, resource, &next_identity).await;
        let cross_identity = send(
            &app,
            "GET",
            &format!("{path}?after_id={id}&after_version=10"),
            &Value::Null,
            StatusCode::OK,
        )
        .await;
        assert_eq!(
            cross_identity,
            json!({
                "definitions": [{"id": "z.next", "version": 1, "name": payload["name"]}],
                "next_after": null
            })
        );
        let scoped = send(
            &app,
            "GET",
            &format!("{path}/{id}/versions?after_id={id}&after_version=10"),
            &Value::Null,
            StatusCode::OK,
        )
        .await;
        assert_eq!(scoped, missing);
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn invalid_cursors_limits_and_methods_are_bounded_and_uncached(pool: PgPool) {
    let app = setup(&pool).await;
    for (resource, payload) in definitions() {
        let path = format!("{ROOT}/{resource}");
        for suffix in [
            "?limit=0",
            "?limit=101",
            "?limit=256",
            "?limit=1&limit=2",
            "?unknown=1",
            "?after_id=id",
            "?after_version=1",
            "?after_id=id&after_version=0",
            "?after_id=&after_version=1",
            "/id/versions?after_id=other&after_version=1",
        ] {
            send(
                &app,
                "GET",
                &format!("{path}{suffix}"),
                &Value::Null,
                StatusCode::BAD_REQUEST,
            )
            .await;
        }
        for (method, uri, allow) in [
            ("PUT", path.clone(), "GET, HEAD, POST"),
            ("POST", format!("{path}/id/versions"), "GET, HEAD"),
        ] {
            let response = app
                .clone()
                .oneshot(request(method, &uri, &payload))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
            assert_eq!(response.headers()[header::ALLOW], allow);
            body(response).await;
        }
        let head = app
            .clone()
            .oneshot(request("HEAD", &path, &Value::Null))
            .await
            .unwrap();
        assert_eq!(head.status(), StatusCode::OK);
        assert_eq!(head.headers()[header::CACHE_CONTROL], "no-store");
        assert!(to_bytes(head.into_body(), 8192).await.unwrap().is_empty());
    }
}

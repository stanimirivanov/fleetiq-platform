//! Versioned snapshots never overwrite prior derived evidence.
use fleetiq_domain::{
    AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, SignalTarget, TenantId,
};
use fleetiq_history::downsampling::{NumericRequest, NumericSpec};
use fleetiq_postgres::{PostgresHistoryBudgets, PostgresNumericSummaries};
use sqlx::PgPool;
fn request() -> NumericRequest {
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
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn summary_revisions_are_exact_immutable_and_retryable(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql(include_str!("property_history/topology.sql"))
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,3,8388608)").execute(&pool).await?;
    let req = request();
    let budgets = PostgresHistoryBudgets::new(pool.clone());
    let fleetiq_history::lifecycle::BudgetAdmission::Reserved(id) =
        budgets.reserve(req.tenant(), 2_097_152).await?
    else {
        panic!("admission");
    };
    let repo = PostgresNumericSummaries::new(pool.clone());
    let first = repo
        .publish(repo.capture(&req, id).await?, &HotOnly)
        .await?;
    let retry = repo
        .publish(repo.capture(&req, id).await?, &HotOnly)
        .await?;
    assert_eq!(first, retry);
    sqlx::query("INSERT INTO history.asset_property_changes VALUES ('tenant-a','message-1','vendor.protocol','1',1,'binding-1',1,'device_recorded',500,1000,150)").execute(&pool).await?;
    let later = repo
        .publish(repo.capture(&req, id).await?, &HotOnly)
        .await?;
    assert_eq!(later.revision, first.revision + 1);
    assert_eq!(later.series, first.series);
    assert!(
        sqlx::query("UPDATE history.numeric_summaries SET observed_at_ms=0")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM history.numeric_summaries")
            .execute(&pool)
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.numeric_summaries")
            .fetch_one(&pool)
            .await?,
        2
    );
    budgets.release(req.tenant(), id).await?;
    Ok(())
}

struct HotOnly;
impl fleetiq_history::downsampling::SummaryArchiveDecoder for HotOnly {
    fn decode(
        &self,
        _tenant: &TenantId,
        _bytes: &[u8],
    ) -> Result<
        Vec<fleetiq_history::StoredPropertyChange>,
        fleetiq_history::downsampling::SummaryFailure,
    > {
        Err(fleetiq_history::downsampling::SummaryFailure::CoverageUnavailable)
    }
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn captured_inputs_are_frozen_and_expired_admission_cannot_publish(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql(include_str!("property_history/topology.sql"))
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,3,8388608)").execute(&pool).await?;
    let req = request();
    let budgets = PostgresHistoryBudgets::new(pool.clone());
    let fleetiq_history::lifecycle::BudgetAdmission::Reserved(id) =
        budgets.reserve(req.tenant(), 2_097_152).await?
    else {
        panic!("admission");
    };
    let repo = PostgresNumericSummaries::new(pool.clone());
    let frozen = repo.capture(&req, id).await?;
    sqlx::query("INSERT INTO history.asset_property_changes VALUES ('tenant-a','message-1','vendor.protocol','1',1,'binding-1',1,'device_recorded',500,1000,150)").execute(&pool).await?;
    let first = repo.publish(frozen, &HotOnly).await?;
    let counts: serde_json::Value = sqlx::query_scalar(
        "SELECT body->'inputs' FROM history.numeric_summaries WHERE series_id=$1 AND revision=$2",
    )
    .bind(first.series)
    .bind(first.revision)
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        counts.as_array().unwrap().len(),
        2,
        "late input never leaks into a captured snapshot"
    );
    let stale = repo.capture(&req, id).await?;
    sqlx::query(
        "UPDATE history.work_reservations SET expires_at=clock_timestamp()-interval '1 second'",
    )
    .execute(&pool)
    .await?;
    assert_eq!(
        repo.publish(stale, &HotOnly).await,
        Err(fleetiq_history::downsampling::SummaryFailure::BudgetDenied)
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.numeric_summaries")
            .fetch_one(&pool)
            .await?,
        1
    );
    Ok(())
}

async fn ready(
    pool: &PgPool,
) -> Result<(NumericRequest, i64, serde_json::Value), Box<dyn std::error::Error>> {
    sqlx::raw_sql(include_str!("property_history/topology.sql"))
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,3,8388608)").execute(pool).await?;
    let req = request();
    let fleetiq_history::lifecycle::BudgetAdmission::Reserved(id) =
        PostgresHistoryBudgets::new(pool.clone())
            .reserve(req.tenant(), 2_097_152)
            .await?
    else {
        panic!("admission");
    };
    let repo = PostgresNumericSummaries::new(pool.clone());
    let receipt = repo
        .publish(repo.capture(&req, id).await?, &HotOnly)
        .await?;
    let mut body: serde_json::Value =
        sqlx::query_scalar("SELECT body FROM history.numeric_summaries WHERE series_id=$1")
            .bind(receipt.series)
            .fetch_one(pool)
            .await?;
    body["buckets"] = serde_json::json!([{"start_ms":"0","end_ms":"3000","total":2,"count":2,"excluded":0,"min":"9223372036854775807","max":"9223372036854775807","sum":"18446744073709551614"}]);
    Ok((req, id, body))
}
async fn direct_revision(
    pool: &PgPool,
    reservation: i64,
    body: &serde_json::Value,
    action: Option<&'static str>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let series:i64=sqlx::query_scalar("INSERT INTO history.numeric_summary_series(tenant_id,asset_id,property_id,property_version,start_ms,end_ms,width_ms) VALUES ('tenant-a','asset-a','electrical.voltage',1,0,3000,3000) RETURNING series_id").fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO history.numeric_summaries(tenant_id,series_id,revision,observed_at_ms,reservation_id,body,fingerprint) VALUES ('tenant-a',$1,1,12345,$2,$3,sha256(convert_to($3::jsonb::text,'UTF8')))").bind(series).bind(reservation).bind(body).execute(&mut *tx).await?;
    sqlx::query("UPDATE history.numeric_summary_series SET revision=1 WHERE series_id=$1")
        .bind(series)
        .execute(&mut *tx)
        .await?;
    if let Some(action) = action {
        sqlx::query(action).execute(&mut *tx).await?;
        tx.commit().await
    } else {
        sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
            .execute(&mut *tx)
            .await?;
        tx.rollback().await
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn malformed_bucket_shapes_never_commit_as_derived_evidence(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (_, id, body) = ready(&pool).await?;
    direct_revision(&pool, id, &body, None).await?;
    for field in ["buckets", "coverage", "kind"] {
        let mut bad = body.clone();
        bad[field] = if field == "buckets" {
            serde_json::json!([null])
        } else {
            serde_json::Value::Null
        };
        assert!(
            direct_revision(&pool, id, &bad, Some("SELECT 1"))
                .await
                .is_err(),
            "invalid {field} must not commit"
        );
    }
    for field in ["start_ms", "end_ms"] {
        let mut bad = body.clone();
        bad["buckets"][0][field] = serde_json::json!(if field == "start_ms" { 0 } else { 3000 });
        assert!(
            direct_revision(&pool, id, &bad, Some("SELECT 1"))
                .await
                .is_err(),
            "numeric {field} must not replace a string"
        );
    }
    let mut incomplete = body.clone();
    incomplete["buckets"] = serde_json::json!([]);
    assert!(
        direct_revision(&pool, id, &incomplete, Some("SELECT 1"))
            .await
            .is_err()
    );
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn commit_rechecks_complete_admission_and_current_tenant_policy(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (_, id, body) = ready(&pool).await?;
    direct_revision(&pool, id, &body, None).await?;
    for action in [
        "UPDATE history.work_reservations SET reserved_bytes=1",
        "INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id) VALUES ('tenant-a',2,'operator-a')",
        "UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'",
    ] {
        assert!(
            direct_revision(&pool, id, &body, Some(action))
                .await
                .is_err(),
            "changed admission must not commit"
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.numeric_summaries")
            .fetch_one(&pool)
            .await?,
        1
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn older_unpublished_snapshot_cannot_replace_more_complete_evidence(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql(include_str!("property_history/topology.sql"))
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,3,8388608)").execute(&pool).await?;
    let req = request();
    let budgets = PostgresHistoryBudgets::new(pool.clone());
    let fleetiq_history::lifecycle::BudgetAdmission::Reserved(id) =
        budgets.reserve(req.tenant(), 2_097_152).await?
    else {
        panic!("admission");
    };
    let repo = PostgresNumericSummaries::new(pool.clone());
    let older = repo.capture(&req, id).await?;
    sqlx::query("INSERT INTO history.asset_property_changes VALUES ('tenant-a','message-1','vendor.protocol','1',1,'binding-1',1,'device_recorded',500,1000,150)").execute(&pool).await?;
    let newest = repo
        .publish(repo.capture(&req, id).await?, &HotOnly)
        .await?;
    assert!(
        repo.publish(older, &HotOnly).await.is_err(),
        "a slower old snapshot must not advance the head after newer evidence"
    );
    let head: i64 = sqlx::query_scalar(
        "SELECT revision FROM history.numeric_summary_series WHERE series_id=$1",
    )
    .bind(newest.series)
    .fetch_one(&pool)
    .await?;
    assert_eq!(head, newest.revision);
    Ok(())
}

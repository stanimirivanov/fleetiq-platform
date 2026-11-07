//! Exact hot reclamation, protected dependencies and retry/restore compatibility.
#[path = "../../archive/tests/support/provider.rs"]
mod provider;
use fleetiq_domain::UnixTimeMillis;
use fleetiq_history::{
    AssetPropertyChangeLog, AssetPropertyChangeView, PropertyChangeAppendOutcome,
    archive_work::{ArchiveObject, ArchiveSegmentStore},
    restore::RestoredSegment,
    retention_work::{RetentionCheck, RetentionRepository, RetentionTarget, RetentionTick},
};
use fleetiq_postgres::{
    PostgresAssetPropertyChangeLog, PostgresAssetPropertyChangeView, PostgresHotRetention,
};
use fleetiq_projection::ProjectionMaintenance;
use sha2::{Digest, Sha256};
use sqlx::{ConnectOptions, PgPool};
#[path = "hot_retention/support.rs"]
mod support;
use support::{quiesce, selection};
#[path = "../../postgres/tests/support/mod.rs"]
mod access_support;
type Error = Box<dyn std::error::Error>;
async fn reclaim_command(
    pool: &PgPool,
    fixture: &provider::Provider,
    target: &RetentionTarget,
    expected: &str,
) -> Result<(), Error> {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_fleetiq-server"))
        .arg("reclaim-once")
        .env_clear()
        .env(
            "DATABASE_URL",
            pool.connect_options().to_url_lossy().as_str(),
        )
        .env("FLEETIQ_RECLAIM_TENANT_ID", target.tenant().as_str())
        .env("FLEETIQ_RECLAIM_MANIFEST_ID", target.manifest().to_string())
        .env(
            "FLEETIQ_RECLAIM_POLICY_REVISION",
            target.revision().to_string(),
        )
        .env("FLEETIQ_ARCHIVE_ENDPOINT", &fixture.endpoint)
        .env("FLEETIQ_ARCHIVE_BUCKET", "archive-test")
        .env("FLEETIQ_ARCHIVE_REGION", "us-east-1")
        .env("FLEETIQ_ARCHIVE_ACCESS_KEY", "archive-fixture-access")
        .env("FLEETIQ_ARCHIVE_SECRET_KEY", "fixture-secret")
        .kill_on_drop(true)
        .output()
        .await?;
    assert!(output.status.success());
    assert_eq!(output.stderr.len(), 0);
    assert_eq!(String::from_utf8(output.stdout)?.trim(), expected);
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn verified_reclamation_preserves_retry_anchors_current_state_and_restore(
    pool: PgPool,
) -> Result<(), Error> {
    let (target, original, bytes) = selection(&pool).await?;
    let fixture = provider::Provider::start().await;
    let objects = fleetiq_archive::objects::ImmutableArchiveObjects::connect(fixture.config())?;
    objects.store(target.tenant(), &original).await?;
    let repo = PostgresHotRetention::new(pool.clone());
    assert!(matches!(
        repo.inspect(&target).await?,
        RetentionCheck::Finished(RetentionTick::Disabled)
    ));
    reclaim_command(&pool, &fixture, &target, "disabled").await?;
    repo.approve(target.tenant(), 1, "operator-a").await?;
    assert_eq!(
        fleetiq_server::reclaim_once(pool.clone(), fixture.config(), &target).await?,
        RetentionTick::Protected,
        "pending projection/discovery protects"
    );
    quiesce(&pool).await?;
    fixture.pause();
    assert!(
        fleetiq_server::reclaim_once(pool.clone(), fixture.config(), &target)
            .await
            .is_err(),
        "provider outage cannot authorize removal"
    );
    fixture.resume();
    let original_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM history.asset_property_changes")
            .fetch_one(&pool)
            .await?;
    let budgets = fleetiq_postgres::PostgresHistoryBudgets::new(pool.clone());
    let fleetiq_history::lifecycle::BudgetAdmission::Reserved(held) =
        budgets.reserve(target.tenant(), 4_194_304).await?
    else {
        panic!("reservation");
    };
    assert_eq!(
        fleetiq_server::reclaim_once(pool.clone(), fixture.config(), &target).await?,
        RetentionTick::Denied
    );
    budgets.release(target.tenant(), held).await?;
    let object = ArchiveObject::new(Sha256::digest(&bytes).into(), i64::try_from(bytes.len())?)?;
    let reference = fleetiq_archive::objects::ArchiveObjectRef::new(
        target.tenant().clone(),
        object.digest(),
        u64::try_from(object.bytes())?,
    )?;
    assert!(
        fixture
            .raw(reqwest::Method::PUT, reference.key(), vec![0; bytes.len()])
            .await?
            .status()
            .is_success()
    );
    assert!(
        fleetiq_server::reclaim_once(pool.clone(), fixture.config(), &target)
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.retention_receipts")
            .fetch_one(&pool)
            .await?,
        0
    );
    assert!(
        fixture
            .raw(reqwest::Method::PUT, reference.key(), bytes)
            .await?
            .status()
            .is_success()
    );
    reclaim_command(&pool, &fixture, &target, "reclaimed 2").await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM history.asset_property_changes WHERE event_at_ms IS NULL"
        )
        .fetch_one(&pool)
        .await?,
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.asset_property_changes")
            .fetch_one(&pool)
            .await?,
        original_count
    );
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM projection.current_properties c JOIN history.asset_property_changes h USING (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis) WHERE h.event_at_ms IS NOT NULL").fetch_one(&pool).await?,1);
    assert!(
        sqlx::query("DELETE FROM history.retention_receipts")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query(
            "UPDATE history.asset_property_changes SET event_at_ms=1000 WHERE event_at_ms IS NULL"
        )
        .execute(&pool)
        .await
        .is_err()
    );
    let view = PostgresAssetPropertyChangeView::new(pool.clone());
    assert!(matches!(
        view.find(target.tenant(), original[0].id()).await,
        Err(fleetiq_postgres::PostgresRepositoryError::ArchivedPayload)
    ));
    // Exact replay uses retained durable source lineage, never reinserts a reclaimed payload.
    let resolver = fleetiq_semantics::SignalBindingResolver::new(
        fleetiq_postgres::PostgresSignalBindingRepository::new(pool.clone()),
        fleetiq_postgres::PostgresDefinitionRepository::new(pool.clone()),
    );
    for before in &original {
        let fleetiq_semantics::ResolutionOutcome::Bound(bound) = resolver
            .resolve(
                before.observation().clone(),
                fleetiq_semantics::ResolutionContext::new(
                    UnixTimeMillis::new(1000),
                    UnixTimeMillis::new(150),
                ),
            )
            .await?
        else {
            panic!("binding");
        };
        let replay =
            fleetiq_semantics::AssetPropertyChange::new(bound, before.id().event_time_basis())?;
        assert_eq!(
            PostgresAssetPropertyChangeLog::new(pool.clone())
                .append(&replay)
                .await?,
            PropertyChangeAppendOutcome::AlreadyPresent
        );
    }
    fleetiq_postgres::PostgresCurrentStateRepository::new(pool.clone())
        .rebuild(target.tenant())
        .await?;
    // A terminal receipt reconciles uncertainty even when the provider is corrupt later.
    assert!(
        fixture
            .raw(
                reqwest::Method::PUT,
                reference.key(),
                vec![0; object.bytes().try_into()?]
            )
            .await?
            .status()
            .is_success()
    );
    reclaim_command(&pool, &fixture, &target, "already-reclaimed 2").await?;
    fixture
        .raw(
            reqwest::Method::PUT,
            reference.key(),
            fleetiq_archive::encode(target.tenant(), &original)?,
        )
        .await?;
    let restores = fleetiq_postgres::PostgresRestore::new(pool.clone());
    let ticket = restores
        .request(target.tenant(), target.manifest(), 0)
        .await?;
    assert_eq!(
        fleetiq_server::restore_once(pool.clone(), fixture.config(), target.tenant(), &ticket)
            .await?,
        fleetiq_history::restore::RestoreTick::Ready
    );
    let staged = restores
        .staged(target.tenant(), &ticket)
        .await?
        .ok_or("missing staging")?;
    assert_eq!(fleetiq_archive::decode(target.tenant(), &staged)?, original);
    fixture.close();
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn provider_success_cannot_bypass_fresh_policy_consumer_or_membership_checks(
    pool: PgPool,
) -> Result<(), Error> {
    let (target, original, bytes) = selection(&pool).await?;
    quiesce(&pool).await?;
    let repo = PostgresHotRetention::new(pool.clone());
    repo.approve(target.tenant(), 1, "operator-a").await?;
    let segment = RestoredSegment::new(bytes.clone(), original.clone())?;
    assert!(matches!(
        repo.inspect(&target).await?,
        RetentionCheck::Verify(_)
    ));
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id) VALUES ('tenant-a',2,'operator-a')").execute(&pool).await?;
    assert_eq!(
        reclaim(&repo, &target, &segment).await?,
        RetentionTick::Disabled
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.retention_receipts")
            .fetch_one(&pool)
            .await?,
        0
    );
    assert!(
        repo.approve(target.tenant(), 1, "operator-a")
            .await
            .is_err()
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn interrupted_transitions_and_concurrent_retries_have_one_receipt(
    pool: PgPool,
) -> Result<(), Error> {
    let (target, original, bytes) = selection(&pool).await?;
    quiesce(&pool).await?;
    let repo = PostgresHotRetention::new(pool.clone());
    repo.approve(target.tenant(), 1, "operator-a").await?;
    let budgets = fleetiq_postgres::PostgresHistoryBudgets::new(pool.clone());
    let fleetiq_history::lifecycle::BudgetAdmission::Reserved(held) =
        budgets.reserve(target.tenant(), 2_097_152).await?
    else {
        panic!("reservation");
    };
    let mut interrupted = pool.begin().await?;
    sqlx::query("INSERT INTO history.retention_receipts(tenant_id,manifest_id,policy_revision,actor_id,records,reservation_id) VALUES ('tenant-a',$1,1,'operator-a',2,$2)").bind(target.manifest()).bind(held).execute(&mut *interrupted).await?;
    assert!(
        interrupted.commit().await.is_err(),
        "receipt without payload transition must not commit"
    );
    budgets.release(target.tenant(), held).await?;
    let segment = RestoredSegment::new(bytes.clone(), original.clone())?;
    assert!(
        reclaim(
            &repo,
            &target,
            &RestoredSegment::new(bytes.clone(), original[..1].to_vec())?
        )
        .await
        .is_err()
    );
    assert!(
        reclaim(
            &repo,
            &target,
            &RestoredSegment::new(vec![1, 2, 3], original)?
        )
        .await
        .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.retention_receipts")
            .fetch_one(&pool)
            .await?,
        0
    );
    let (a, b) = tokio::join!(
        reclaim(&repo, &target, &segment),
        reclaim(&repo, &target, &segment)
    );
    assert!(matches!(
        (a?, b?),
        (
            RetentionTick::Reclaimed(2),
            RetentionTick::AlreadyReclaimed(2)
        ) | (
            RetentionTick::AlreadyReclaimed(2),
            RetentionTick::Reclaimed(2)
        )
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.retention_receipts")
            .fetch_one(&pool)
            .await?,
        1
    );
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn fresh_restore_revocation_and_calculator_holds_block_verified_bytes(
    pool: PgPool,
) -> Result<(), Error> {
    let (target, original, bytes) = selection(&pool).await?;
    quiesce(&pool).await?;
    let repo = PostgresHotRetention::new(pool.clone());
    repo.approve(target.tenant(), 1, "operator-a").await?;
    let segment = RestoredSegment::new(bytes, original)?;
    assert!(matches!(
        repo.inspect(&target).await?,
        RetentionCheck::Verify(_)
    ));
    let restores = fleetiq_postgres::PostgresRestore::new(pool.clone());
    let ticket = restores
        .request(target.tenant(), target.manifest(), 0)
        .await?;
    assert_eq!(
        reclaim(&repo, &target, &segment).await?,
        RetentionTick::Protected
    );
    restores.cancel(target.tenant(), &ticket).await?;
    repo.revoke(target.tenant()).await?;
    assert_eq!(
        reclaim(&repo, &target, &segment).await?,
        RetentionTick::Disabled
    );
    repo.approve(target.tenant(), 1, "operator-a").await?;
    sqlx::query(
        "INSERT INTO analytics.prepared_evaluations VALUES ('tenant-a','late-consumer','{}','{}')",
    )
    .execute(&pool)
    .await?;
    assert_eq!(
        reclaim(&repo, &target, &segment).await?,
        RetentionTick::Protected
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.retention_receipts")
            .fetch_one(&pool)
            .await?,
        0
    );
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn dependency_creation_is_excluded_inside_the_retention_transaction(
    pool: PgPool,
) -> Result<(), Error> {
    let (target, _, _) = selection(&pool).await?;
    quiesce(&pool).await?;
    let repo = PostgresHotRetention::new(pool.clone());
    repo.approve(target.tenant(), 1, "operator-a").await?;
    let mut critical = pool.begin().await?;
    sqlx::query("SELECT history.lock_retention_dependencies()")
        .execute(&mut *critical)
        .await?;
    let mut writer = pool.begin().await?;
    sqlx::query("SET LOCAL lock_timeout='1ms'")
        .execute(&mut *writer)
        .await?;
    let error=sqlx::query("INSERT INTO history.restore_jobs(tenant_id,manifest_id,request_generation) VALUES ('tenant-a',$1,1)").bind(target.manifest()).execute(&mut *writer).await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("55P03"),
        "a new dependency cannot cross the check/mutation boundary"
    );
    writer.rollback().await?;
    critical.rollback().await?;
    assert_eq!(
        fleetiq_postgres::PostgresRestore::new(pool.clone())
            .request(target.tenant(), target.manifest(), 0)
            .await?
            .manifest(),
        target.manifest()
    );
    Ok(())
}

async fn calculator_key(pool: &PgPool) -> Result<serde_json::Value, Error> {
    sqlx::raw_sql("INSERT INTO metadata.property_definitions(tenant_id,property_id,version,display_name,value_kind,canonical_unit) VALUES ('tenant-a','retention.mean',1,'Mean','decimal','volt'); INSERT INTO analytics.calculator_definitions VALUES ('tenant-a','retention-mean','1','window-mean-v1','electrical.voltage',1,'retention.mean',1)").execute(pool).await?;
    let evidence:String=sqlx::query_scalar("SELECT analytics.semantic_evidence_id(tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis) FROM history.asset_property_changes WHERE output_index=0 AND event_time_basis='device_recorded'").fetch_one(pool).await?;
    Ok(
        serde_json::json!({"scope":"asset-a","calculator":"retention-mean","version":"1","configuration":"retention-mean-config","start":0,"end":3000,"finalize":3000,"known":3000,"inputs":[[evidence,1000]]}),
    )
}
async fn append_terminal(pool: &PgPool, key: &serde_json::Value) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO analytics.evaluations(tenant_id,evaluation_id,replay_key,key_digest,minimum_samples,evaluated_at_ms,outcome,outputs) VALUES ('tenant-a','retention-evaluation',$1,sha256(convert_to($1::jsonb::text,'UTF8')),2,3500,'insufficient','[]')").bind(key).execute(pool).await?;
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn terminal_calculator_references_pin_exact_archived_members(
    pool: PgPool,
) -> Result<(), Error> {
    let (target, original, bytes) = selection(&pool).await?;
    quiesce(&pool).await?;
    let repo = PostgresHotRetention::new(pool.clone());
    repo.approve(target.tenant(), 1, "operator-a").await?;
    let key = calculator_key(&pool).await?;
    append_terminal(&pool, &key).await?;
    assert_eq!(
        reclaim(&repo, &target, &RestoredSegment::new(bytes, original)?).await?,
        RetentionTick::Protected
    );
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn new_calculator_inputs_cannot_exploit_archived_null_comparisons(
    pool: PgPool,
) -> Result<(), Error> {
    let (target, original, bytes) = selection(&pool).await?;
    quiesce(&pool).await?;
    let repo = PostgresHotRetention::new(pool.clone());
    repo.approve(target.tenant(), 1, "operator-a").await?;
    let key = calculator_key(&pool).await?;
    assert_eq!(
        reclaim(&repo, &target, &RestoredSegment::new(bytes, original)?).await?,
        RetentionTick::Reclaimed(2)
    );
    let error = append_terminal(&pool, &key).await.unwrap_err();
    assert!(
        error.to_string().contains("archived calculator evidence"),
        "{error}"
    );
    Ok(())
}

async fn reclaim(
    repo: &PostgresHotRetention,
    target: &fleetiq_history::retention_work::RetentionTarget,
    segment: &RestoredSegment,
) -> Result<RetentionTick, fleetiq_history::retention_work::RetentionFailure> {
    repo.reclaim(target, &support::Source(segment)).await
}

async fn preview(
    pool: &PgPool,
    target: &fleetiq_history::retention_work::RetentionTarget,
) -> Result<fleetiq_history::lifecycle::retention::RetentionPreview, Error> {
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-a','operations.read') ON CONFLICT DO NOTHING").execute(pool).await?;
    let grant = fleetiq_access::TenantAuthorizationService::new(
        fleetiq_postgres::PostgresTenantAccessPolicy::new(pool.clone()),
    )
    .authorize::<fleetiq_access::ReadOperations>(
        access_support::authenticated_actor("operator-a").await,
        target.tenant().clone(),
    )
    .await?;
    Ok(
        fleetiq_postgres::PostgresRetentionPreview::new(pool.clone())
            .snapshot(grant)
            .await?,
    )
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn manifest_preview_propagates_unsampled_pins_without_duplicate_reasons(
    pool: PgPool,
) -> Result<(), Error> {
    use fleetiq_history::lifecycle::EvidenceProtection;
    let (target, _, _) = selection(&pool).await?;
    support::drain(&pool).await?;
    let key = calculator_key(&pool).await?;
    append_terminal(&pool, &key).await?;
    let report = preview(&pool, &target).await?;
    let archived = report
        .candidates
        .iter()
        .filter(|c| c.archive_verified)
        .collect::<Vec<_>>();
    assert_eq!(archived.len(), 2);
    for candidate in archived {
        assert_eq!(
            candidate
                .protections
                .iter()
                .filter(|p| **p == EvidenceProtection::CurrentProjection)
                .count(),
            1
        );
        assert_eq!(
            candidate
                .protections
                .iter()
                .filter(|p| **p == EvidenceProtection::PinnedAnalytics)
                .count(),
            1
        );
        assert!(!candidate.payload_eligible());
    }
    // Put 99 new identities before the sampled manifest member, moving the
    // pinned current sibling outside the hundred-row sample.
    sqlx::raw_sql("INSERT INTO ingestion.accepted_messages(tenant_id,message_id,device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload) SELECT tenant_id,'a-earlier',device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload FROM ingestion.accepted_messages;
      INSERT INTO ingestion.decoded_message_results VALUES ('tenant-a','a-earlier','vendor.protocol','1',99);
      INSERT INTO ingestion.decoded_observations SELECT tenant_id,'a-earlier',protocol_package_id,protocol_package_version,output_index,endpoint_id,signal_id,device_recorded_at_ms,value_kind,value_boolean,value_integer,value_decimal,value_text,source_unit,quality FROM ingestion.decoded_observations WHERE message_id='message-1' AND output_index<99").execute(&pool).await?;
    support::drain(&pool).await?;
    let report = preview(&pool, &target).await?;
    assert_eq!(report.candidates.len(), 100);
    let sampled = report
        .candidates
        .iter()
        .filter(|c| c.archive_verified)
        .collect::<Vec<_>>();
    assert_eq!(sampled.len(), 1);
    assert!(
        sampled[0]
            .protections
            .contains(&EvidenceProtection::CurrentProjection)
    );
    assert_eq!(sampled[0].estimated_reclaimable_bytes(), 0);
    assert!(report.has_more);
    Ok(())
}

#[path = "hot_retention/isolation.rs"]
mod isolation;

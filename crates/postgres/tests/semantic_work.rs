//! Real database contracts: ownership, frozen decisions, atomicity and recovery.
#[path = "semantic_work/support.rs"]
mod support;
use fleetiq_history::{
    DecodedObservationView, SemanticTick, SemanticWorkRepository, SemanticWorker,
};
use fleetiq_metadata::SignalBindingRepository;
use fleetiq_postgres::{
    PostgresDecodedObservationView, PostgresDefinitionRepository, PostgresSemanticWorkRepository,
    PostgresSignalBindingRepository,
};
use sqlx::PgPool;
use std::error::Error;
use support::{binding, change, setup};

type TestWorker = SemanticWorker<
    PostgresSemanticWorkRepository,
    PostgresDecodedObservationView,
    PostgresSignalBindingRepository,
    PostgresDefinitionRepository,
>;
fn worker(pool: &PgPool) -> TestWorker {
    SemanticWorker::new(
        PostgresSemanticWorkRepository::new(pool.clone()),
        PostgresDecodedObservationView::new(pool.clone()),
        PostgresSignalBindingRepository::new(pool.clone()),
        PostgresDefinitionRepository::new(pool.clone()),
    )
}
async fn status(pool: &PgPool) -> Result<String, sqlx::Error> {
    sqlx::query_scalar("SELECT status FROM history.semantic_work ORDER BY message_id LIMIT 1")
        .fetch_one(pool)
        .await
}
async fn count(pool: &PgPool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM history.asset_property_changes")
        .fetch_one(pool)
        .await
}
async fn expire(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE history.semantic_work SET lease_until_ms=0 WHERE status='pending'")
        .execute(pool)
        .await?;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn committed_source_is_processed_once_with_explicit_time_choices(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool, true).await?;
    let (basis, known): (String, i64) =
        sqlx::query_as("SELECT event_time_basis,known_at_ms FROM history.semantic_work")
            .fetch_one(&pool)
            .await?;
    assert_eq!(basis, "device_recorded");
    assert!(known > 100);
    assert_eq!(worker(&pool).run_once().await?, SemanticTick::Completed);
    assert_eq!(worker(&pool).run_once().await?, SemanticTick::Idle);
    assert_eq!(count(&pool).await?, 1);
    assert_eq!(status(&pool).await?, "completed");
    let (event, cutoff): (i64, i64) = sqlx::query_as(
        "SELECT event_at_ms,resolution_known_at_ms FROM history.asset_property_changes",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!((event, cutoff), (1000, known));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn selected_revision_survives_restart_and_later_correction(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let source = setup(&pool, true).await?;
    let repo = PostgresSemanticWorkRepository::new(pool.clone());
    let lease = repo.claim().await?.unwrap();
    let original = change(
        &pool,
        source,
        lease.request.known_at().get(),
        lease.request.basis(),
    )
    .await?;
    assert!(!repo.complete(&lease, &original).await?);
    assert_eq!(count(&pool).await?, 0);
    assert!(repo.pin(&lease, &original).await?);
    PostgresSignalBindingRepository::new(pool.clone())
        .insert_revision(&binding(
            2,
            lease.request.known_at().get() + 1,
            "power-system-2",
        )?)
        .await?;
    assert!(
        sqlx::query("UPDATE history.semantic_work SET binding_revision=2")
            .execute(&pool)
            .await
            .is_err()
    );
    expire(&pool).await?;
    assert!(!repo.complete(&lease, &original).await?);
    // A newly constructed worker has no in-memory retry state.
    assert_eq!(worker(&pool).run_once().await?, SemanticTick::Completed);
    assert_eq!(count(&pool).await?, 1);
    let revision: bigdecimal::BigDecimal =
        sqlx::query_scalar("SELECT binding_revision FROM history.asset_property_changes")
            .fetch_one(&pool)
            .await?;
    assert_eq!(revision, bigdecimal::BigDecimal::from(1));
    assert!(!repo.complete(&lease, &original).await?);
    assert_eq!(worker(&pool).run_once().await?, SemanticTick::Idle);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn competing_claims_are_exclusive_and_stale_generations_are_fenced(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let source = setup(&pool, true).await?;
    let repo = PostgresSemanticWorkRepository::new(pool.clone());
    let (left, right) = tokio::join!(repo.claim(), repo.claim());
    let leases = [left?, right?];
    assert_eq!(leases.iter().flatten().count(), 1);
    let old = leases.into_iter().flatten().next().unwrap();
    let candidate = change(
        &pool,
        source,
        old.request.known_at().get(),
        old.request.basis(),
    )
    .await?;
    assert!(repo.pin(&old, &candidate).await?);
    expire(&pool).await?;
    let new = repo.claim().await?.unwrap();
    assert_eq!(new.generation, old.generation + 1);
    assert_eq!(new.request, old.request);
    assert_eq!(new.selected.as_ref().unwrap().revision().get(), 1);
    assert!(!repo.pin(&old, &candidate).await?);
    assert!(!repo.complete(&old, &candidate).await?);
    assert!(repo.complete(&new, &candidate).await?);
    // Simulate lost acknowledgement after commit: exact old completion cannot append again.
    assert!(!repo.complete(&new, &candidate).await?);
    assert_eq!(count(&pool).await?, 1);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn append_failure_rolls_back_terminal_progress_and_retries_saved_selection(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let source = setup(&pool, true).await?;
    let repo = PostgresSemanticWorkRepository::new(pool.clone());
    let lease = repo.claim().await?.unwrap();
    let candidate = change(
        &pool,
        source,
        lease.request.known_at().get(),
        lease.request.basis(),
    )
    .await?;
    assert!(repo.pin(&lease, &candidate).await?);
    sqlx::raw_sql("CREATE FUNCTION public.fail_semantic_append() RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic failure'; END; $$; CREATE TRIGGER fail_append AFTER INSERT ON history.asset_property_changes FOR EACH ROW EXECUTE FUNCTION public.fail_semantic_append();")
        .execute(&pool).await?;
    assert!(repo.complete(&lease, &candidate).await.is_err());
    assert_eq!(count(&pool).await?, 0);
    assert_eq!(status(&pool).await?, "pending");
    sqlx::query("DROP TRIGGER fail_append ON history.asset_property_changes")
        .execute(&pool)
        .await?;
    expire(&pool).await?;
    assert_eq!(worker(&pool).run_once().await?, SemanticTick::Completed);
    assert_eq!(count(&pool).await?, 1);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn suspension_blocks_claim_and_inflight_completion_until_reactivation(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let source = setup(&pool, true).await?;
    let repo = PostgresSemanticWorkRepository::new(pool.clone());
    let lease = repo.claim().await?.unwrap();
    let candidate = change(
        &pool,
        source,
        lease.request.known_at().get(),
        lease.request.basis(),
    )
    .await?;
    assert!(repo.pin(&lease, &candidate).await?);
    sqlx::query("UPDATE metadata.tenants SET is_active=FALSE")
        .execute(&pool)
        .await?;
    assert!(!repo.complete(&lease, &candidate).await?);
    assert_eq!(count(&pool).await?, 0);
    expire(&pool).await?;
    assert!(repo.claim().await?.is_none());
    sqlx::query("UPDATE metadata.tenants SET is_active=TRUE")
        .execute(&pool)
        .await?;
    assert_eq!(worker(&pool).run_once().await?, SemanticTick::Completed);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn unresolved_source_is_retained_without_automatic_reinterpretation(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let source = setup(&pool, false).await?;
    assert_eq!(worker(&pool).run_once().await?, SemanticTick::Unresolved);
    assert_eq!(status(&pool).await?, "unresolved");
    let reason: String = sqlx::query_scalar("SELECT reason FROM history.semantic_work")
        .fetch_one(&pool)
        .await?;
    assert_eq!(reason, "no_binding");
    PostgresSignalBindingRepository::new(pool.clone())
        .insert_revision(&binding(1, 100, "power-system-1")?)
        .await?;
    assert_eq!(worker(&pool).run_once().await?, SemanticTick::Idle);
    assert_eq!(count(&pool).await?, 0);
    assert_eq!(
        PostgresDecodedObservationView::new(pool.clone())
            .find(source.tenant_id(), source.id())
            .await?,
        Some(source)
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn abandoned_final_attempt_is_retained_and_time_policy_cannot_change(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool, true).await?;
    let repo = PostgresSemanticWorkRepository::new(pool.clone());
    for sql in [
        "UPDATE history.semantic_work SET known_at_ms=known_at_ms+1",
        "UPDATE history.semantic_work SET event_time_basis='received_at'",
        "UPDATE history.semantic_work SET status='unresolved'",
    ] {
        assert!(sqlx::query(sql).execute(&pool).await.is_err());
    }
    for generation in 1..=3 {
        assert_eq!(repo.claim().await?.unwrap().generation, generation);
        expire(&pool).await?;
    }
    assert!(repo.claim().await?.is_none());
    assert_eq!(status(&pool).await?, "exhausted");
    for sql in [
        "UPDATE history.semantic_work SET known_at_ms=known_at_ms+1",
        "UPDATE history.semantic_work SET event_time_basis='received_at'",
        "UPDATE history.semantic_work SET status='pending'",
        "DELETE FROM history.semantic_work",
    ] {
        assert!(sqlx::query(sql).execute(&pool).await.is_err());
    }
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn source_rollback_rolls_back_handoff_and_foreign_source_is_rejected(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool, true).await?;
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO ingestion.decoded_observations SELECT tenant_id,message_id,protocol_package_id,protocol_package_version,1,endpoint_id,signal_id,device_recorded_at_ms,value_kind,value_boolean,value_integer,value_decimal,value_text,source_unit,quality FROM ingestion.decoded_observations WHERE output_index=0")
        .execute(&mut *tx).await?;
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM history.semantic_work")
        .fetch_one(&mut *tx)
        .await?;
    assert_eq!(pending, 2);
    tx.rollback().await?;
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM history.semantic_work")
        .fetch_one(&pool)
        .await?;
    assert_eq!(pending, 1);
    assert!(sqlx::query("INSERT INTO history.semantic_work (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,event_time_basis,known_at_ms) VALUES ('tenant-b','message-1','vendor.protocol','1',0,'received_at',0)")
        .execute(&pool).await.is_err());
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn pre_migration_discovery_has_bounded_checkpoint_and_receipt_fallback(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    // Model legacy source rows with only the new handoff trigger disabled;
    // all historical source constraints and immutable facts remain active.
    sqlx::query(
        "ALTER TABLE ingestion.decoded_observations DISABLE TRIGGER enqueue_semantic_observation",
    )
    .execute(&pool)
    .await?;
    setup(&pool, true).await?;
    sqlx::raw_sql("INSERT INTO ingestion.accepted_messages SELECT tenant_id, 'old-'||n, device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload FROM ingestion.accepted_messages CROSS JOIN generate_series(1,65) n; INSERT INTO ingestion.decoded_message_results SELECT tenant_id,message_id,protocol_package_id,protocol_package_version,1 FROM ingestion.accepted_messages WHERE message_id<>'message-1'; INSERT INTO ingestion.decoded_observations (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,endpoint_id,signal_id,value_kind,value_integer,source_unit,quality) SELECT tenant_id,message_id,protocol_package_id,protocol_package_version,0,'can-primary','bus.voltage','integer',48,'volt','good' FROM ingestion.accepted_messages WHERE message_id<>'message-1';")
        .execute(&pool).await?;
    sqlx::query(
        "ALTER TABLE ingestion.decoded_observations ENABLE TRIGGER enqueue_semantic_observation",
    )
    .execute(&pool)
    .await?;
    let repo = PostgresSemanticWorkRepository::new(pool.clone());
    for expected in [32_i64, 64, 66, 66] {
        repo.seed().await?;
        let actual: i64 = sqlx::query_scalar("SELECT count(*) FROM history.semantic_work")
            .fetch_one(&pool)
            .await?;
        assert_eq!(actual, expected);
    }
    let complete: bool = sqlx::query_scalar("SELECT completed FROM history.semantic_backfill")
        .fetch_one(&pool)
        .await?;
    assert!(complete);
    let basis: String = sqlx::query_scalar(
        "SELECT event_time_basis FROM history.semantic_work WHERE message_id='old-1'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(basis, "received_at");
    assert_eq!(worker(&pool).run_once().await?, SemanticTick::Completed);
    assert_eq!(worker(&pool).run_once().await?, SemanticTick::Completed);
    let fallback: i64 = sqlx::query_scalar(
        "SELECT event_at_ms FROM history.asset_property_changes WHERE message_id='old-1'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(fallback, 1100);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn already_durable_interpretation_keeps_first_append_context(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    use fleetiq_history::AssetPropertyChangeLog;
    use fleetiq_postgres::PostgresAssetPropertyChangeLog;
    use fleetiq_semantics::EventTimeBasis;
    let source = setup(&pool, true).await?;
    let original = change(&pool, source, 150, EventTimeBasis::DeviceRecorded).await?;
    PostgresAssetPropertyChangeLog::new(pool.clone())
        .append(&original)
        .await?;
    assert_eq!(worker(&pool).run_once().await?, SemanticTick::Completed);
    assert_eq!(count(&pool).await?, 1);
    let first: i64 =
        sqlx::query_scalar("SELECT resolution_known_at_ms FROM history.asset_property_changes")
            .fetch_one(&pool)
            .await?;
    assert_eq!(first, 150);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn saved_selection_does_not_repeat_candidate_discovery_after_restart(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    use fleetiq_domain::{
        EffectiveInterval, RecordRevision, RecordedRevision, SignalBinding, SignalBindingId,
        UnixTimeMillis,
    };
    let source = setup(&pool, true).await?;
    let repo = PostgresSemanticWorkRepository::new(pool.clone());
    let lease = repo.claim().await?.unwrap();
    let original = change(
        &pool,
        source.clone(),
        lease.request.known_at().get(),
        lease.request.basis(),
    )
    .await?;
    assert!(repo.pin(&lease, &original).await?);
    let first = binding(1, 100, "power-system-1")?;
    let competitor = SignalBinding::new(
        first.tenant_id().clone(),
        SignalBindingId::new("binding-2")?,
        RecordedRevision::new(RecordRevision::new(1)?, UnixTimeMillis::new(100)),
        first.source().clone(),
        first.target().clone(),
        EffectiveInterval::new(UnixTimeMillis::new(0), None)?,
    );
    PostgresSignalBindingRepository::new(pool.clone())
        .insert_revision(&competitor)
        .await?;
    // A frozen cutoff alone is insufficient: this separate, backdated identity
    // now makes ordinary candidate discovery ambiguous at the original cutoff.
    assert!(
        change(
            &pool,
            source,
            lease.request.known_at().get(),
            lease.request.basis()
        )
        .await
        .is_err()
    );
    expire(&pool).await?;
    assert_eq!(worker(&pool).run_once().await?, SemanticTick::Completed);
    assert_eq!(count(&pool).await?, 1);
    let selected: String =
        sqlx::query_scalar("SELECT binding_id FROM history.asset_property_changes")
            .fetch_one(&pool)
            .await?;
    assert_eq!(selected, "binding-1");
    Ok(())
}

#[path = "semantic_work/planning.rs"]
mod planning;

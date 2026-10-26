//! The operational read model observes durable state without claiming process health.
use super::*;
use fleetiq_ingestion::{IngestionDiagnosticsRepository, LiveReplayRepository, ProcessingEvidence};
use fleetiq_postgres::PostgresIngestionDiagnostics;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn diagnostics_distinguish_pending_leased_expired_poisoned_and_empty_decodes(pool: PgPool) {
    let base = seed(&pool).await;
    let repo = PostgresReplayRepository::new(pool.clone());
    let mut expected = Vec::new();
    for (index, evidence) in [
        ProcessingEvidence::Queued,
        ProcessingEvidence::Leased,
        ProcessingEvidence::Expired,
        ProcessingEvidence::Poisoned,
        ProcessingEvidence::Decoded,
    ]
    .into_iter()
    .enumerate()
    {
        let mut work = base.clone();
        work.session = format!("session-{index}");
        work.message = AcceptedMessage::new(
            AcceptedMessageId::new(format!("message-{index}")).unwrap(),
            base.message.tenant_id().clone(),
            base.message.device_asset_id().clone(),
            base.message.protocol_package().clone(),
            UnixTimeMillis::new(i64::try_from(index).unwrap()),
            base.message.payload().clone(),
        );
        assert!(repo.enqueue(&work).await.unwrap());
        if evidence != ProcessingEvidence::Queued {
            let lease = repo.claim_exact(&work.message).await.unwrap().unwrap();
            match evidence {
                ProcessingEvidence::Expired => expire(&pool, &lease).await,
                ProcessingEvidence::Poisoned => assert!(repo.poison(&lease).await.unwrap()),
                ProcessingEvidence::Decoded => {
                    let (batch, state) = ReplayEffects {
                        outputs: vec![],
                        state: vec![1],
                    }
                    .batch(&work)
                    .unwrap();
                    assert!(repo.complete(&lease, &batch, &state).await.unwrap());
                }
                _ => {}
            }
        }
        expected.push(evidence);
    }
    let read = PostgresIngestionDiagnostics::new(pool.clone());
    let snapshot = read
        .snapshot(base.message.tenant_id(), base.message.device_asset_id())
        .await
        .unwrap()
        .unwrap();
    expected.reverse();
    assert_eq!(
        snapshot
            .frames
            .iter()
            .map(|f| f.processing)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(snapshot.frames[0].observation_count, Some(0));
    assert_eq!(snapshot.frames[1].observation_count, None);
    assert_eq!(snapshot.sources, []);
    let foreign = read
        .snapshot(
            &TenantId::new("tenant-b").unwrap(),
            base.message.device_asset_id(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(foreign.frames, []);
    assert!(
        read.snapshot(base.message.tenant_id(), &AssetId::new("missing").unwrap())
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn diagnostics_bound_history_output_prefixes_and_distinct_source_coverage(pool: PgPool) {
    let work = seed(&pool).await;
    // Realistic log cardinality: 5,000 receipts and 1,200 decoded positions per
    // selected frame. Handwritten fixture SQL is not production persistence.
    sqlx::raw_sql("INSERT INTO ingestion.accepted_messages(tenant_id,message_id,device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload) SELECT 'tenant-a','sample-'||n,'device-a','fixture.custom','1',n,'private-wire'::bytea FROM generate_series(1,5000) n;
        INSERT INTO ingestion.decoded_message_results(tenant_id,message_id,protocol_package_id,protocol_package_version,observation_count) SELECT 'tenant-a','sample-'||n,'fixture.custom','1',1200 FROM generate_series(4937,5000) n;
        INSERT INTO ingestion.decoded_observations(tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,endpoint_id,signal_id,value_kind,value_text,quality) SELECT 'tenant-a','sample-'||n,'fixture.custom','1',p,'main','signal-'||n||'-'||p,'text','private-value','good' FROM generate_series(4937,5000) n CROSS JOIN generate_series(0,1199) p;").execute(&pool).await.unwrap();
    let read = PostgresIngestionDiagnostics::new(pool.clone());
    let snapshot = read
        .snapshot(work.message.tenant_id(), work.message.device_asset_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.frames.len(), 64);
    assert_eq!(snapshot.sources.len(), 128);
    assert!(
        snapshot.frames_truncated && snapshot.observations_truncated && snapshot.sources_truncated
    );
    assert_eq!(snapshot.frames.first().unwrap().received_at_ms, 5000);
    assert_eq!(snapshot.frames.last().unwrap().received_at_ms, 4937);
    for source in &snapshot.sources {
        let position = source
            .signal_id
            .rsplit('-')
            .next()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert!(position < 64, "source outside inspected prefix");
        assert_eq!(source.package_id, "fixture.custom");
    }
    let second = read
        .snapshot(work.message.tenant_id(), work.message.device_asset_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.frames, second.frames);
    assert_eq!(snapshot.sources, second.sources);
    // The matching ordered index supports a bounded seek, not a historical sort.
    sqlx::query("ANALYZE ingestion.accepted_messages")
        .execute(&pool)
        .await
        .unwrap();
    let plan=sqlx::query_scalar::<_,String>("EXPLAIN SELECT message_id FROM ingestion.accepted_messages WHERE tenant_id='tenant-a' AND device_asset_id='device-a' ORDER BY received_at_ms DESC,message_id DESC LIMIT 65")
        .fetch_all(&pool).await.unwrap().join("\n");
    assert!(
        plan.contains("ix_accepted_messages_device_recent"),
        "{plan}"
    );
    assert!(!plan.contains("Sort"), "{plan}");
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn diagnostics_keep_colliding_tenant_message_ids_separate_and_deduplicate_sources(
    pool: PgPool,
) {
    let work = seed(&pool).await;
    sqlx::raw_sql("INSERT INTO ingestion.accepted_messages(tenant_id,message_id,device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload) VALUES
        ('tenant-a','shared-1','device-a','fixture.custom','1',1,'wire-a'::bytea),
        ('tenant-a','shared-2','device-a','fixture.custom','1',2,'wire-a'::bytea),
        ('tenant-a','raw-only','device-a','fixture.custom','1',3,'wire-a'::bytea),
        ('tenant-b','shared-1','device-a','fixture.custom','1',9999,'wire-b'::bytea);
        INSERT INTO ingestion.decoded_message_results(tenant_id,message_id,protocol_package_id,protocol_package_version,observation_count) VALUES
        ('tenant-a','shared-1','fixture.custom','1',1),('tenant-a','shared-2','fixture.custom','1',1),('tenant-b','shared-1','fixture.custom','1',1);
        INSERT INTO ingestion.decoded_observations(tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,endpoint_id,signal_id,value_kind,value_text,quality) VALUES
        ('tenant-a','shared-1','fixture.custom','1',0,'main','battery','text','private-a','good'),
        ('tenant-a','shared-2','fixture.custom','1',0,'main','battery','text','private-a','good'),
        ('tenant-b','shared-1','fixture.custom','1',0,'foreign','foreign-signal','text','private-b','good');").execute(&pool).await.unwrap();
    let read = PostgresIngestionDiagnostics::new(pool);
    let snapshot = read
        .snapshot(work.message.tenant_id(), work.message.device_asset_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.frames.len(), 3);
    assert_eq!(snapshot.frames[0].message_id, "raw-only");
    assert_eq!(snapshot.frames[0].processing, ProcessingEvidence::Accepted);
    assert_eq!(snapshot.sources.len(), 1);
    assert_eq!(snapshot.sources[0].endpoint_id, "main");
    assert_eq!(snapshot.sources[0].signal_id, "battery");
    assert!(
        !snapshot.frames_truncated
            && !snapshot.observations_truncated
            && !snapshot.sources_truncated
    );
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn diagnostics_do_not_hide_pending_or_poisoned_replay_behind_a_legacy_decode_marker(
    pool: PgPool,
) {
    let work = seed(&pool).await;
    let replay = PostgresReplayRepository::new(pool.clone());
    assert!(replay.enqueue(&work).await.unwrap());
    // The older decoded-log port can have persisted output before stateful
    // replay/checkpoint processing. This is valid durable evidence, not proof
    // that the newly queued worker turn has completed.
    sqlx::query("INSERT INTO ingestion.decoded_message_results(tenant_id,message_id,protocol_package_id,protocol_package_version,observation_count) VALUES('tenant-a','message-1','fixture.custom','1',0)")
        .execute(&pool).await.unwrap();
    let read = PostgresIngestionDiagnostics::new(pool);
    let queued = read
        .snapshot(work.message.tenant_id(), work.message.device_asset_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(queued.frames[0].processing, ProcessingEvidence::Queued);
    assert_eq!(queued.frames[0].observation_count, Some(0));
    let lease = replay.claim_exact(&work.message).await.unwrap().unwrap();
    assert!(replay.poison(&lease).await.unwrap());
    let poisoned = read
        .snapshot(work.message.tenant_id(), work.message.device_asset_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(poisoned.frames[0].processing, ProcessingEvidence::Poisoned);
    assert_eq!(poisoned.frames[0].observation_count, Some(0));
}

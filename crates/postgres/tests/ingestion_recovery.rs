//! Recovery across the real host, application transitions, and PostgreSQL logs.
//!
//! Fault decorators report failure before an append or suppress its successful
//! response after commit. This deterministically models ambiguous durability;
//! it does not simulate socket failure, process termination, or power loss.

use std::error::Error;

use fleetiq_application::{FrameAcceptanceError, ObservationPersistenceError};
use fleetiq_postgres::{PostgresAcceptedMessageLog, PostgresDecodedObservationLog};
use sqlx::PgPool;

#[path = "ingestion_recovery/support.rs"]
mod support;

use support::{Fault, FaultLog, counts, prepared_frame, seed_device};

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn failed_raw_append_retries_without_releasing_deferred_effects(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    recover_raw_append(pool, Fault::BeforeAppend).await
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn lost_raw_append_response_resolves_as_exact_retry(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    recover_raw_append(pool, Fault::AfterAppend).await
}

async fn recover_raw_append(pool: PgPool, fault: Fault) -> Result<(), Box<dyn Error>> {
    seed_device(&pool).await?;
    let (ready, pending) = prepared_frame(2)?.into_parts();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].as_bytes(), b"ready");
    assert!(ready[0].durable_acceptance().is_none());
    let original = pending.clone();

    let error = pending
        .accept(&FaultLog::new(pool.clone(), fault))
        .await
        .expect_err("the injected failure cannot authorize durable effects");
    let FrameAcceptanceError::Append { source, pending } = error else {
        panic!("the decorator must report an append failure");
    };
    assert_eq!(source.kind(), std::io::ErrorKind::ConnectionReset);
    assert_eq!(*pending, original);
    assert_eq!(pending.deferred_reply_count(), 1);
    assert_eq!(pending.decoded_output_count(), 2);
    let was_committed = matches!(fault, Fault::AfterAppend);
    assert_eq!(counts(&pool).await?, (i64::from(was_committed), 0, 0));

    let accepted = pending
        .accept(&PostgresAcceptedMessageLog::new(pool.clone()))
        .await?;
    assert_eq!(accepted.append_outcome().was_appended(), !was_committed);
    assert_eq!(accepted.released_replies().len(), 1);
    assert_eq!(accepted.released_replies()[0].as_bytes(), b"accepted");
    assert_eq!(
        accepted.released_replies()[0].durable_acceptance(),
        Some(accepted.message().id())
    );
    let persisted = accepted
        .prepare_observation_persistence()?
        .persist(&PostgresDecodedObservationLog::new(pool.clone()))
        .await?;
    assert!(persisted.observation_append_outcome().was_appended());
    support::assert_observations(&persisted, 2);
    assert_eq!(counts(&pool).await?, (1, 1, 2));
    support::assert_stored_observations(&pool).await?;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn lost_decoded_append_response_preserves_exact_batch_and_released_replies(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    recover_decoded_append(pool, 2).await
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn lost_empty_decoded_append_response_preserves_completion_evidence(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    recover_decoded_append(pool, 0).await
}

async fn recover_decoded_append(pool: PgPool, output_count: u8) -> Result<(), Box<dyn Error>> {
    seed_device(&pool).await?;
    let (_, pending) = prepared_frame(output_count)?.into_parts();
    let accepted = pending
        .accept(&PostgresAcceptedMessageLog::new(pool.clone()))
        .await?;
    let pending = accepted.prepare_observation_persistence()?;
    let original = pending.clone();

    let error = pending
        .persist(&FaultLog::new(pool.clone(), Fault::AfterAppend))
        .await
        .expect_err("a suppressed commit response must remain retryable");
    let ObservationPersistenceError::Append { source, pending } = error;
    assert_eq!(source.kind(), std::io::ErrorKind::ConnectionReset);
    assert_eq!(*pending, original);
    assert_eq!(pending.observation_count(), usize::from(output_count));
    assert!(pending.accepted_message_was_appended());
    assert_eq!(pending.released_replies()[0].as_bytes(), b"accepted");
    assert_eq!(
        pending.released_replies()[0].durable_acceptance(),
        Some(pending.message().id())
    );
    assert_eq!(counts(&pool).await?, (1, 1, i64::from(output_count)));

    let persisted = pending
        .persist(&PostgresDecodedObservationLog::new(pool.clone()))
        .await?;
    assert!(persisted.accepted_message_was_appended());
    assert!(!persisted.observation_append_outcome().was_appended());
    support::assert_observations(&persisted, output_count);
    assert_eq!(counts(&pool).await?, (1, 1, i64::from(output_count)));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn repeated_processed_delivery_preserves_ids_and_does_not_duplicate_rows(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_device(&pool).await?;
    let raw_log = PostgresAcceptedMessageLog::new(pool.clone());
    let decoded_log = PostgresDecodedObservationLog::new(pool.clone());
    let (_, pending) = prepared_frame(2)?.into_parts();
    let retry = pending.clone();
    let first = pending
        .accept(&raw_log)
        .await?
        .prepare_observation_persistence()?
        .persist(&decoded_log)
        .await?;
    let repeated = retry
        .accept(&raw_log)
        .await?
        .prepare_observation_persistence()?
        .persist(&decoded_log)
        .await?;
    assert!(first.accepted_message_was_appended());
    assert!(first.observation_append_outcome().was_appended());
    assert!(!repeated.accepted_message_was_appended());
    assert!(!repeated.observation_append_outcome().was_appended());
    assert_eq!(first.batch(), repeated.batch());
    assert_eq!(first.released_replies(), repeated.released_replies());
    assert_eq!(counts(&pool).await?, (1, 1, 2));
    Ok(())
}

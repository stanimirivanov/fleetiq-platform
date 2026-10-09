//! PostgreSQL behavioral tests for single-use browser login and session evidence.

use std::error::Error;

use fleetiq_domain::{
    ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId, UnixTimeMillis,
};
use fleetiq_postgres::{
    BrowserLoginAttempt, BrowserSession, PostgresBrowserIdentityRepository, PostgresRepositoryError,
};
use sqlx::PgPool;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn login_attempt_round_trip_is_bound_to_browser_and_consumed_once(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let repository = PostgresBrowserIdentityRepository::new(pool);
    let attempt = login_attempt('a', 2_000);
    repository.insert_login_attempt(&attempt).await?;

    assert!(
        repository
            .consume_login_attempt(&attempt.state_digest, &digest('c'), instant(1_000))
            .await?
            .is_none()
    );
    assert_eq!(
        repository
            .consume_login_attempt(
                &attempt.state_digest,
                &attempt.browser_digest,
                instant(1_999)
            )
            .await?,
        Some(attempt.clone())
    );
    assert!(
        repository
            .consume_login_attempt(
                &attempt.state_digest,
                &attempt.browser_digest,
                instant(1_999)
            )
            .await?
            .is_none()
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_login_consumption_has_exactly_one_winner(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let repository = PostgresBrowserIdentityRepository::new(pool);
    let attempt = login_attempt('a', 2_000);
    repository.insert_login_attempt(&attempt).await?;

    let (first, second) = tokio::join!(
        repository.consume_login_attempt(
            &attempt.state_digest,
            &attempt.browser_digest,
            instant(1_000)
        ),
        repository.consume_login_attempt(
            &attempt.state_digest,
            &attempt.browser_digest,
            instant(1_000)
        ),
    );
    let first = first?;
    let second = second?;
    assert_eq!(
        usize::from(first.is_some()) + usize::from(second.is_some()),
        1
    );
    assert_eq!(first.or(second), Some(attempt));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn login_deadline_is_exclusive_and_cleanup_removes_retained_secrets(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let repository = PostgresBrowserIdentityRepository::new(pool.clone());
    let attempt = login_attempt('a', 2_000);
    repository.insert_login_attempt(&attempt).await?;

    assert!(
        repository
            .consume_login_attempt(
                &attempt.state_digest,
                &attempt.browser_digest,
                instant(2_000)
            )
            .await?
            .is_none()
    );
    repository.delete_expired(instant(2_000)).await?;
    assert_eq!(row_count(&pool, EvidenceTable::LoginAttempts).await?, 0);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn session_preserves_exact_identity_and_expires_at_its_deadline(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_actor(&pool).await?;
    let repository = PostgresBrowserIdentityRepository::new(pool);
    let session = session('a', 2_000);
    repository.insert_session(&session).await?;

    assert_eq!(
        repository
            .find_session(&session.credential_digest, instant(1_999))
            .await?,
        Some(session.clone())
    );
    assert!(
        repository
            .find_session(&session.credential_digest, instant(2_000))
            .await?
            .is_none()
    );
    assert!(
        repository
            .find_session(&digest('c'), instant(1_000))
            .await?
            .is_none()
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn logout_deletes_only_the_exact_session_and_is_repeatable(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_actor(&pool).await?;
    let repository = PostgresBrowserIdentityRepository::new(pool);
    let first = session('a', 2_000);
    let second = session('c', 2_000);
    repository.insert_session(&first).await?;
    repository.insert_session(&second).await?;

    repository.delete_session(&first.credential_digest).await?;
    repository.delete_session(&first.credential_digest).await?;
    assert!(
        repository
            .find_session(&first.credential_digest, instant(1_000))
            .await?
            .is_none()
    );
    assert_eq!(
        repository
            .find_session(&second.credential_digest, instant(1_000))
            .await?,
        Some(second)
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn logout_cancels_only_the_matching_browsers_pending_login_attempts(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let repository = PostgresBrowserIdentityRepository::new(pool);
    let first = login_attempt('a', 2_000);
    let second = login_attempt('c', 2_000);
    let mut other_browser = login_attempt('d', 2_000);
    other_browser.browser_digest = digest('e');
    repository.insert_login_attempt(&first).await?;
    repository.insert_login_attempt(&second).await?;
    repository.insert_login_attempt(&other_browser).await?;

    repository
        .delete_login_attempts(&first.browser_digest)
        .await?;
    repository
        .delete_login_attempts(&first.browser_digest)
        .await?;
    for cancelled in [first, second] {
        assert!(
            repository
                .consume_login_attempt(
                    &cancelled.state_digest,
                    &cancelled.browser_digest,
                    instant(1_000)
                )
                .await?
                .is_none()
        );
    }
    assert_eq!(
        repository
            .consume_login_attempt(
                &other_browser.state_digest,
                &other_browser.browser_digest,
                instant(1_000)
            )
            .await?,
        Some(other_browser)
    );
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn cleanup_deletes_both_expired_tables_and_preserves_unexpired_evidence(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_actor(&pool).await?;
    let repository = PostgresBrowserIdentityRepository::new(pool.clone());
    let expired_attempt = login_attempt('a', 1_000);
    let active_attempt = login_attempt('c', 1_001);
    let expired_session = session('a', 1_000);
    let active_session = session('c', 1_001);
    repository.insert_login_attempt(&expired_attempt).await?;
    repository.insert_login_attempt(&active_attempt).await?;
    repository.insert_session(&expired_session).await?;
    repository.insert_session(&active_session).await?;

    repository.delete_expired(instant(1_000)).await?;
    repository.delete_expired(instant(1_000)).await?;
    assert_eq!(row_count(&pool, EvidenceTable::LoginAttempts).await?, 1);
    assert_eq!(row_count(&pool, EvidenceTable::Sessions).await?, 1);
    assert_eq!(
        repository
            .find_session(&active_session.credential_digest, instant(1_000))
            .await?,
        Some(active_session)
    );
    assert_eq!(
        repository
            .consume_login_attempt(
                &active_attempt.state_digest,
                &active_attempt.browser_digest,
                instant(1_000)
            )
            .await?,
        Some(active_attempt)
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn durable_login_constraints_reject_unbounded_or_malformed_evidence(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let repository = PostgresBrowserIdentityRepository::new(pool);
    for (field, value, expected) in [
        (
            "state",
            digest('A'),
            "ck_browser_login_attempts_state_digest",
        ),
        (
            "provider",
            digest('A'),
            "ck_browser_login_attempts_provider_digest",
        ),
        (
            "browser",
            "raw-cookie".to_owned(),
            "ck_browser_login_attempts_browser_digest",
        ),
        (
            "verifier",
            "short".to_owned(),
            "ck_browser_login_attempts_pkce_verifier",
        ),
        (
            "verifier",
            "a".repeat(129),
            "ck_browser_login_attempts_pkce_verifier",
        ),
        (
            "nonce",
            "short".to_owned(),
            "ck_browser_login_attempts_nonce",
        ),
        ("nonce", "a".repeat(257), "ck_browser_login_attempts_nonce"),
    ] {
        let mut attempt = login_attempt('a', 2_000);
        match field {
            "state" => attempt.state_digest = value,
            "provider" => attempt.provider_digest = value,
            "browser" => attempt.browser_digest = value,
            "verifier" => attempt.pkce_verifier = value,
            "nonce" => attempt.nonce = value,
            _ => unreachable!("test cases define valid fields"),
        }
        assert_invariant(
            &repository.insert_login_attempt(&attempt).await.unwrap_err(),
            expected,
        );
    }
    for (state, nonce_length, verifier_length) in [('a', 16, 43), ('c', 256, 128)] {
        let mut valid = login_attempt(state, 2_000);
        valid.nonce = "a".repeat(nonce_length);
        valid.pkce_verifier = "a".repeat(verifier_length);
        repository.insert_login_attempt(&valid).await?;
    }
    let attempt = login_attempt('e', 0);
    assert_invariant(
        &repository.insert_login_attempt(&attempt).await.unwrap_err(),
        "ck_browser_login_attempts_expiry",
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn session_constraints_require_digest_expiry_and_registered_actor(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_actor(&pool).await?;
    let repository = PostgresBrowserIdentityRepository::new(pool);
    let mut invalid = session('A', 2_000);
    assert_invariant(
        &repository.insert_session(&invalid).await.unwrap_err(),
        "ck_browser_sessions_credential_digest",
    );
    invalid = session('a', 2_000);
    invalid.provider_digest = digest('A');
    assert_invariant(
        &repository.insert_session(&invalid).await.unwrap_err(),
        "ck_browser_sessions_provider_digest",
    );
    invalid = session('a', 0);
    assert_invariant(
        &repository.insert_session(&invalid).await.unwrap_err(),
        "ck_browser_sessions_expiry",
    );
    invalid = session('a', 2_000);
    invalid.actor_id = ActorId::new("actor-missing")?;
    assert!(matches!(
        repository.insert_session(&invalid).await.unwrap_err(),
        PostgresRepositoryError::InvalidReference { constraint: Some(name), .. }
            if name == "fk_browser_sessions_actor"
    ));

    let valid = session('a', 2_000);
    repository.insert_session(&valid).await?;
    assert!(matches!(
        repository.insert_session(&valid).await.unwrap_err(),
        PostgresRepositoryError::Conflict { constraint: Some(name), .. }
            if name == "pk_browser_sessions"
    ));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn authentication_evidence_cannot_be_updated_to_extend_expiry(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_actor(&pool).await?;
    let repository = PostgresBrowserIdentityRepository::new(pool.clone());
    repository
        .insert_login_attempt(&login_attempt('a', 2_000))
        .await?;
    repository.insert_session(&session('a', 2_000)).await?;

    for statement in [
        "UPDATE identity.browser_login_attempts SET expires_at_ms = 3000",
        "UPDATE identity.browser_sessions SET expires_at_ms = 3000",
    ] {
        let error = sqlx::query(statement).execute(&pool).await.unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(sqlx::error::DatabaseError::constraint),
            Some("ck_browser_identity_immutable")
        );
    }
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn persisted_subject_is_revalidated_even_if_database_constraints_are_bypassed(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_actor(&pool).await?;
    let repository = PostgresBrowserIdentityRepository::new(pool.clone());
    let evidence = session('a', 2_000);
    repository.insert_session(&evidence).await?;

    // Only this isolated test database permits damage simulation. Production
    // callers cannot replace identity evidence or bypass these constraints.
    sqlx::query(
        "ALTER TABLE identity.browser_sessions DROP CONSTRAINT ck_browser_sessions_subject",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "ALTER TABLE identity.browser_sessions DISABLE TRIGGER trg_browser_sessions_immutable",
    )
    .execute(&pool)
    .await?;
    sqlx::query("UPDATE identity.browser_sessions SET subject = ''")
        .execute(&pool)
        .await?;
    assert!(matches!(
        repository
            .find_session(&evidence.credential_digest, instant(1_000))
            .await
            .unwrap_err(),
        PostgresRepositoryError::CorruptData {
            field: "identity.browser_sessions.subject",
            ..
        }
    ));
    Ok(())
}

#[test]
fn debug_output_never_contains_authentication_or_private_login_material() {
    let attempt = login_attempt('a', 2_000);
    let evidence = session('a', 2_000);
    let attempt_debug = format!("{attempt:?}");
    let session_debug = format!("{evidence:?}");
    for private in [
        &attempt.state_digest,
        &attempt.provider_digest,
        &attempt.browser_digest,
        &attempt.pkce_verifier,
        &attempt.nonce,
    ] {
        assert!(!attempt_debug.contains(private));
    }
    for private in [
        evidence.credential_digest.as_str(),
        evidence.provider_digest.as_str(),
        evidence.identity.subject().as_str(),
        evidence.actor_id.as_str(),
    ] {
        assert!(!session_debug.contains(private));
    }
}

fn digest(character: char) -> String {
    character.to_string().repeat(64)
}

fn instant(value: i64) -> UnixTimeMillis {
    UnixTimeMillis::new(value)
}

fn login_attempt(character: char, expires_at_ms: i64) -> BrowserLoginAttempt {
    BrowserLoginAttempt {
        state_digest: digest(character),
        provider_digest: digest('d'),
        browser_digest: digest('b'),
        pkce_verifier: "private_pkce_verifier_".repeat(3),
        nonce: "private_oidc_nonce_123456".to_owned(),
        expires_at: instant(expires_at_ms),
    }
}

fn session(character: char, expires_at_ms: i64) -> BrowserSession {
    BrowserSession {
        credential_digest: digest(character),
        provider_digest: digest('d'),
        identity: ExternalIdentity::new(
            IdentityAuthorityId::new("workforce-oidc").expect("valid test authority"),
            ExternalIdentitySubject::new(" Subject-A/Ä ").expect("valid exact opaque test subject"),
        ),
        actor_id: ActorId::new("operator-a").expect("valid test actor"),
        expires_at: instant(expires_at_ms),
    }
}

async fn seed_actor(pool: &PgPool) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ('operator-a')")
        .execute(pool)
        .await?;
    Ok(())
}

enum EvidenceTable {
    LoginAttempts,
    Sessions,
}

async fn row_count(pool: &PgPool, table: EvidenceTable) -> sqlx::Result<i64> {
    let query = match table {
        EvidenceTable::LoginAttempts => {
            sqlx::query_scalar("SELECT count(*) FROM identity.browser_login_attempts")
        }
        EvidenceTable::Sessions => {
            sqlx::query_scalar("SELECT count(*) FROM identity.browser_sessions")
        }
    };
    query.fetch_one(pool).await
}

fn assert_invariant(error: &PostgresRepositoryError, expected: &str) {
    assert!(matches!(
        error,
        PostgresRepositoryError::InvariantViolation { constraint: Some(name), .. }
            if name == expected
    ));
}

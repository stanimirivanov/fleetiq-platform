//! Durable, single-use browser login attempts and opaque operator sessions.

use std::fmt;

use fleetiq_domain::{
    ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId, UnixTimeMillis,
};
use sqlx::PgPool;

use crate::{
    PostgresRepositoryError,
    error::{classify_database_error, corrupt},
};

/// Server-owned evidence required to complete one browser-bound OIDC login.
///
/// Only hashes of the browser's state and binding credentials are stored. The
/// PKCE verifier remains backend-only during token exchange. State and nonce
/// are provider authorization parameters carried in the redirect; authentication
/// material must never enter routine diagnostics.
#[derive(Clone, Eq, PartialEq)]
pub struct BrowserLoginAttempt {
    /// Lowercase hexadecimal SHA-256 digest of the OIDC state credential.
    pub state_digest: String,
    /// Stable SHA-256 binding of trusted issuer, client, and authority configuration.
    ///
    /// The callback rejects attempts created under a different trust domain.
    pub provider_digest: String,
    /// Lowercase hexadecimal SHA-256 digest of the browser binding cookie.
    pub browser_digest: String,
    /// Server-held RFC 7636 verifier, consumed once during token exchange.
    pub pkce_verifier: String,
    /// OIDC nonce correlated with the redirect and verified identity token.
    pub nonce: String,
    /// Exclusive login completion deadline in Unix milliseconds.
    pub expires_at: UnixTimeMillis,
}

impl fmt::Debug for BrowserLoginAttempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BrowserLoginAttempt")
            .field("credentials", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// Immutable server session issued after a trusted operator sign-in.
///
/// A session captures authentication evidence, never tenant authorization.
/// Consumers must re-resolve the external identity to the same active actor
/// and enforce current grants on every protected request. No provider tokens
/// or raw session cookie credentials are persisted.
#[derive(Clone, Eq, PartialEq)]
pub struct BrowserSession {
    /// Lowercase hexadecimal SHA-256 digest of the opaque session credential.
    pub credential_digest: String,
    /// Stable SHA-256 binding of trusted issuer, client, and authority configuration.
    ///
    /// Consumers reject sessions when this trust domain changes. Client secrets
    /// are excluded so ordinary secret rotation preserves valid sessions.
    pub provider_digest: String,
    /// Exact authenticated authority and provider subject.
    pub identity: ExternalIdentity,
    /// Actor to which the verified identity resolved when this session began.
    pub actor_id: ActorId,
    /// Exclusive session authentication deadline in Unix milliseconds.
    pub expires_at: UnixTimeMillis,
}

impl fmt::Debug for BrowserSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BrowserSession")
            .field("authentication", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// PostgreSQL storage for browser login and session authentication evidence.
///
/// Rows are immutable after insertion and expire at an explicit caller-supplied
/// instant. This adapter performs no OIDC verification or authorization.
#[derive(Clone, Debug)]
pub struct PostgresBrowserIdentityRepository {
    pool: PgPool,
}

impl PostgresBrowserIdentityRepository {
    /// Creates browser identity storage using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Persists one bounded login attempt without storing raw cookie credentials.
    ///
    /// # Errors
    ///
    /// Returns a conflict for reused state identity, an invariant violation for
    /// invalid digests, verifier, nonce, or expiry, or a database failure.
    pub async fn insert_login_attempt(
        &self,
        attempt: &BrowserLoginAttempt,
    ) -> Result<(), PostgresRepositoryError> {
        sqlx::query!(
            r#"
            INSERT INTO identity.browser_login_attempts
                (state_digest, provider_digest, browser_digest, pkce_verifier, nonce, expires_at_ms)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            attempt.state_digest,
            attempt.provider_digest,
            attempt.browser_digest,
            attempt.pkce_verifier,
            attempt.nonce,
            attempt.expires_at.get(),
        )
        .execute(&self.pool)
        .await
        .map_err(classify_database_error)?;
        Ok(())
    }

    /// Atomically consumes a nonexpired attempt bound to the exact browser.
    ///
    /// A single `DELETE RETURNING` statement makes concurrent callbacks compete
    /// for one row: only one receives the verifier. Wrong-browser and expired
    /// callbacks return `None` without consuming a still-valid attempt. Once the
    /// database completes deletion, retrying cannot recover the attempt; callers
    /// restart sign-in after interrupted token exchange rather than replay it.
    ///
    /// # Errors
    ///
    /// Returns a database failure when consumption cannot be completed.
    pub async fn consume_login_attempt(
        &self,
        state_digest: &str,
        browser_digest: &str,
        now: UnixTimeMillis,
    ) -> Result<Option<BrowserLoginAttempt>, PostgresRepositoryError> {
        let row = sqlx::query_as!(
            LoginAttemptRow,
            r#"
            DELETE FROM identity.browser_login_attempts
            WHERE state_digest = $1 AND browser_digest = $2 AND expires_at_ms > $3
            RETURNING state_digest, provider_digest, browser_digest, pkce_verifier, nonce, expires_at_ms
            "#,
            state_digest,
            browser_digest,
            now.get(),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?;
        Ok(row.map(LoginAttemptRow::into_attempt))
    }

    /// Inserts immutable authentication evidence for an existing internal actor.
    ///
    /// The external mapping deliberately has no foreign key: unlinking or
    /// reassigning an identity must remain possible to revoke authentication.
    /// The consuming boundary rechecks that mapping on every request.
    ///
    /// # Errors
    ///
    /// Returns a conflict for a reused credential digest, an invalid reference
    /// for a missing actor, an invariant violation, or a database failure.
    pub async fn insert_session(
        &self,
        session: &BrowserSession,
    ) -> Result<(), PostgresRepositoryError> {
        sqlx::query!(
            r#"
            INSERT INTO identity.browser_sessions
                (credential_digest, provider_digest, authority_id, subject, actor_id, expires_at_ms)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            session.credential_digest,
            session.provider_digest,
            session.identity.authority_id().as_str(),
            session.identity.subject().as_str(),
            session.actor_id.as_str(),
            session.expires_at.get(),
        )
        .execute(&self.pool)
        .await
        .map_err(classify_database_error)?;
        Ok(())
    }

    /// Finds nonexpired authentication evidence by its hashed cookie credential.
    ///
    /// The deadline is exclusive: a session is absent at its expiry instant.
    /// A returned row proves neither current actor activity nor tenant access.
    ///
    /// # Errors
    ///
    /// Returns a database failure or corrupt-data error when persisted identity
    /// values cannot be reconstructed through the domain constructors.
    pub async fn find_session(
        &self,
        credential_digest: &str,
        now: UnixTimeMillis,
    ) -> Result<Option<BrowserSession>, PostgresRepositoryError> {
        sqlx::query_as!(
            SessionRow,
            r#"
            SELECT credential_digest, provider_digest, authority_id, subject, actor_id, expires_at_ms
            FROM identity.browser_sessions
            WHERE credential_digest = $1 AND expires_at_ms > $2
            "#,
            credential_digest,
            now.get(),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?
        .map(SessionRow::reconstruct)
        .transpose()
    }

    /// Revokes an exact session; repeating deletion succeeds if already absent.
    ///
    /// # Errors
    ///
    /// Returns a database failure when revocation cannot be completed.
    pub async fn delete_session(
        &self,
        credential_digest: &str,
    ) -> Result<(), PostgresRepositoryError> {
        sqlx::query!(
            "DELETE FROM identity.browser_sessions WHERE credential_digest = $1",
            credential_digest,
        )
        .execute(&self.pool)
        .await
        .map_err(classify_database_error)?;
        Ok(())
    }

    /// Cancels all still-pending login attempts bound to an exact browser cookie.
    ///
    /// Logout uses this deletion alongside session revocation so clearing the
    /// cookie alone cannot leave a pending callback usable. Repeated cancellation
    /// succeeds. A callback that already consumed its attempt is in flight and
    /// cannot be undone by this operation; callers must document that race.
    ///
    /// # Errors
    ///
    /// Returns a database failure when pending login cancellation cannot complete.
    pub async fn delete_login_attempts(
        &self,
        browser_digest: &str,
    ) -> Result<(), PostgresRepositoryError> {
        sqlx::query!(
            "DELETE FROM identity.browser_login_attempts WHERE browser_digest = $1",
            browser_digest,
        )
        .execute(&self.pool)
        .await
        .map_err(classify_database_error)?;
        Ok(())
    }
    /// Removes expired login secrets and session evidence in one atomic statement.
    ///
    /// Cleanup is safe to repeat and never extends authentication lifetimes.
    /// Both indexed deletions share the same explicit cutoff and commit together.
    ///
    /// # Errors
    ///
    /// Returns a database failure when cleanup cannot be completed.
    pub async fn delete_expired(&self, now: UnixTimeMillis) -> Result<(), PostgresRepositoryError> {
        sqlx::query!(
            r#"
            WITH expired_attempts AS (
                DELETE FROM identity.browser_login_attempts WHERE expires_at_ms <= $1
            )
            DELETE FROM identity.browser_sessions WHERE expires_at_ms <= $1
            "#,
            now.get(),
        )
        .execute(&self.pool)
        .await
        .map_err(classify_database_error)?;
        Ok(())
    }
}

struct LoginAttemptRow {
    state_digest: String,
    provider_digest: String,
    browser_digest: String,
    pkce_verifier: String,
    nonce: String,
    expires_at_ms: i64,
}

impl LoginAttemptRow {
    fn into_attempt(self) -> BrowserLoginAttempt {
        BrowserLoginAttempt {
            state_digest: self.state_digest,
            provider_digest: self.provider_digest,
            browser_digest: self.browser_digest,
            pkce_verifier: self.pkce_verifier,
            nonce: self.nonce,
            expires_at: UnixTimeMillis::new(self.expires_at_ms),
        }
    }
}

struct SessionRow {
    credential_digest: String,
    provider_digest: String,
    authority_id: String,
    subject: String,
    actor_id: String,
    expires_at_ms: i64,
}

impl SessionRow {
    fn reconstruct(self) -> Result<BrowserSession, PostgresRepositoryError> {
        Ok(BrowserSession {
            credential_digest: self.credential_digest,
            provider_digest: self.provider_digest,
            identity: ExternalIdentity::new(
                IdentityAuthorityId::new(self.authority_id)
                    .map_err(|error| corrupt("identity.browser_sessions.authority_id", error))?,
                ExternalIdentitySubject::new(self.subject)
                    .map_err(|error| corrupt("identity.browser_sessions.subject", error))?,
            ),
            actor_id: ActorId::new(self.actor_id)
                .map_err(|error| corrupt("identity.browser_sessions.actor_id", error))?,
            expires_at: UnixTimeMillis::new(self.expires_at_ms),
        })
    }
}

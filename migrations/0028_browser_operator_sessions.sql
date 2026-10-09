-- ============================================================================
-- FleetIQ browser-bound login attempts and opaque operator sessions
-- ============================================================================
--
-- Adds short-lived, server-owned OIDC login evidence and authenticated browser
-- sessions. These rows are platform identity data, not tenant-owned records:
-- one operator may hold grants in several tenants, and each protected request
-- still resolves the current actor and evaluates its exact tenant grant.
--
-- Only SHA-256 digests of raw browser credentials are persisted. PKCE verifiers
-- stay backend-only for one bounded token exchange. State and nonce are provider
-- authorization parameters and must not appear in routine diagnostics. Provider
-- access, refresh, and identity tokens are never stored here. The
-- application database role must remain private to the trusted server. No
-- browser or direct tenant database role receives access to these tables.
--
-- Rows can be inserted or deleted but never updated: expiry cannot slide and
-- an authenticated identity cannot be silently replaced. Deletion implements
-- single-use login consumption, logout, and periodic expiry cleanup. This is
-- disposable authentication evidence, not an immutable business audit trail.
--
-- Expiry uses signed BIGINT Unix milliseconds to match the existing domain
-- UnixTimeMillis and explicit clock boundary exactly, without floating-point
-- or timestamp conversions. Only positive future-era deadlines are stored;
-- at expires_at_ms <= the supplied current instant, evidence is invalid.
--
-- Rollout is additive and transactional. New empty tables and their indexes
-- cause no existing-data rewrite or backfill. Old binaries ignore them. After
-- rollback, rows expire naturally and can be removed by the next deployment.
-- ============================================================================


-- ============================================================================
-- Single-use browser-bound login evidence
-- ============================================================================

-- The state digest locates a pending callback. The separate browser digest
-- proves that the callback came from the browser that initiated sign-in; wrong
-- browsers cannot consume another browser's verifier merely by learning state.
CREATE TABLE identity.browser_login_attempts (
    state_digest TEXT NOT NULL,
    -- A callback must retain the same issuer, client, and authority trust domain
    -- used at initiation before sending the code to a configured token endpoint.
    provider_digest TEXT NOT NULL,
    browser_digest TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    nonce TEXT NOT NULL,
    expires_at_ms BIGINT NOT NULL,

    CONSTRAINT pk_browser_login_attempts PRIMARY KEY (state_digest),

    CONSTRAINT ck_browser_login_attempts_state_digest
        CHECK (state_digest ~ '^[0-9a-f]{64}$'),

    CONSTRAINT ck_browser_login_attempts_provider_digest
        CHECK (provider_digest ~ '^[0-9a-f]{64}$'),

    CONSTRAINT ck_browser_login_attempts_browser_digest
        CHECK (browser_digest ~ '^[0-9a-f]{64}$'),

    -- RFC 7636 bounds the verifier to 43..128 unreserved ASCII characters.
    CONSTRAINT ck_browser_login_attempts_pkce_verifier
        CHECK (pkce_verifier ~ '^[A-Za-z0-9._~-]{43,128}$'),

    -- FleetIQ generates an opaque base64url nonce locally. Length is checked
    -- separately because PostgreSQL ARE repetition bounds stop at 255; the
    -- supported 256-character limit must not be encoded as a regex quantifier.
    CONSTRAINT ck_browser_login_attempts_nonce
        CHECK (
            length(nonce) BETWEEN 16 AND 256
            AND nonce ~ '^[A-Za-z0-9_-]+$'
        ),

    CONSTRAINT ck_browser_login_attempts_expiry CHECK (expires_at_ms > 0)
);

-- Cleanup needs the expiry order independently of the state primary key.
CREATE INDEX ix_browser_login_attempts_expiry
ON identity.browser_login_attempts (expires_at_ms);

-- Logout cancels pending attempts by their browser binding, independently of
-- state lookup and expiry cleanup. The referencing digest needs its own index
-- so this deletion does not scan every operator's short-lived login evidence.
CREATE INDEX ix_browser_login_attempts_browser
ON identity.browser_login_attempts (browser_digest);


-- ============================================================================
-- Opaque browser session evidence
-- ============================================================================

-- The actor is the resolved identity at sign-in time, not a cached authorization
-- decision. There is deliberately no foreign key to external_actor_identities:
-- an administrator must be able to unlink or reassign that mapping to revoke
-- access. The request boundary re-resolves authority+subject and compares the
-- resulting active actor with this row before accepting authentication.
CREATE TABLE identity.browser_sessions (
    credential_digest TEXT NOT NULL,
    -- Binds evidence to configured issuer, client, and authority. Excludes the
    -- client secret so rotating that credential does not reset operator sessions.
    provider_digest TEXT NOT NULL,
    authority_id TEXT NOT NULL,
    subject TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    expires_at_ms BIGINT NOT NULL,

    CONSTRAINT pk_browser_sessions PRIMARY KEY (credential_digest),

    -- Actors retain their stable identities through deactivation. RESTRICT
    -- prevents deleting identity evidence by an incidental cascading mutation;
    -- session deletion is an explicit lifecycle operation.
    CONSTRAINT fk_browser_sessions_actor
        FOREIGN KEY (actor_id) REFERENCES identity.actors (actor_id)
        ON DELETE RESTRICT,

    CONSTRAINT ck_browser_sessions_credential_digest
        CHECK (credential_digest ~ '^[0-9a-f]{64}$'),

    CONSTRAINT ck_browser_sessions_provider_digest
        CHECK (provider_digest ~ '^[0-9a-f]{64}$'),

    CONSTRAINT ck_browser_sessions_authority
        CHECK (metadata.is_valid_identifier(authority_id)),

    -- Subjects remain exact, opaque, and case-sensitive, matching the existing
    -- external identity domain constructor rather than normalizing identity.
    CONSTRAINT ck_browser_sessions_subject
        CHECK (
            char_length(subject) BETWEEN 1 AND 512
            AND btrim(subject) <> ''
            AND subject !~ '[[:cntrl:]]'
        ),

    CONSTRAINT ck_browser_sessions_actor
        CHECK (metadata.is_valid_identifier(actor_id)),

    CONSTRAINT ck_browser_sessions_expiry CHECK (expires_at_ms > 0)
);

-- Cleanup uses expiry independently of cookie lookup, and PostgreSQL does not
-- automatically index the referencing side of the stable actor foreign key.
CREATE INDEX ix_browser_sessions_expiry
ON identity.browser_sessions (expires_at_ms);

CREATE INDEX ix_browser_sessions_actor
ON identity.browser_sessions (actor_id);


-- ============================================================================
-- Immutable authentication evidence; explicit deletion remains allowed
-- ============================================================================

-- Prevents extending a deadline or replacing the identity captured at login.
-- Logout and one-time consumption use DELETE and therefore remain available.
CREATE FUNCTION identity.reject_browser_identity_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'browser authentication evidence cannot be updated'
        USING ERRCODE = '23514',
              CONSTRAINT = 'ck_browser_identity_immutable';
END;
$$;

CREATE TRIGGER trg_browser_login_attempts_immutable
BEFORE UPDATE ON identity.browser_login_attempts
FOR EACH ROW EXECUTE FUNCTION identity.reject_browser_identity_update();

CREATE TRIGGER trg_browser_sessions_immutable
BEFORE UPDATE ON identity.browser_sessions
FOR EACH ROW EXECUTE FUNCTION identity.reject_browser_identity_update();


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON TABLE identity.browser_login_attempts IS
    'Disposable single-use server login evidence bound to one initiating browser; retains backend-only PKCE material and nonce correlation evidence until consumption or expiry cleanup.';
COMMENT ON COLUMN identity.browser_login_attempts.state_digest IS
    'Lowercase SHA-256 hex digest of the state credential; raw browser credentials must never be persisted.';
COMMENT ON COLUMN identity.browser_login_attempts.provider_digest IS
    'Stable SHA-256 binding of the trusted OIDC issuer, client, and authority at initiation; callbacks reject changed trust domains before token exchange.';
COMMENT ON COLUMN identity.browser_login_attempts.browser_digest IS
    'Lowercase SHA-256 hex digest of the browser binding cookie required alongside state for atomic consumption.';
COMMENT ON COLUMN identity.browser_login_attempts.pkce_verifier IS
    'Private RFC 7636 verifier retained only until one token exchange; never expose through logs or browser responses.';
COMMENT ON COLUMN identity.browser_login_attempts.nonce IS
    'Locally generated OIDC nonce sent in the provider authorization redirect and verified against the returned identity token; omit from routine diagnostics.';
COMMENT ON COLUMN identity.browser_login_attempts.expires_at_ms IS
    'Exclusive positive Unix millisecond login deadline; cleanup removes rows at or before its supplied cutoff.';

COMMENT ON TABLE identity.browser_sessions IS
    'Disposable immutable server authentication evidence for opaque browser credentials; never a substitute for current identity resolution or tenant authorization.';
COMMENT ON COLUMN identity.browser_sessions.credential_digest IS
    'Lowercase SHA-256 hex digest of the random opaque session cookie; no raw session credential or provider token is stored.';
COMMENT ON COLUMN identity.browser_sessions.provider_digest IS
    'Stable SHA-256 binding of trusted OIDC issuer, client, and authority; a changed trust domain invalidates prior sessions, while client-secret rotation preserves them.';
COMMENT ON COLUMN identity.browser_sessions.authority_id IS
    'Trusted FleetIQ authentication configuration identity established by verified OIDC sign-in.';
COMMENT ON COLUMN identity.browser_sessions.subject IS
    'Exact opaque provider subject; request authentication must re-resolve its current mapping to the recorded actor.';
COMMENT ON COLUMN identity.browser_sessions.actor_id IS
    'Stable actor resolved at sign-in; current mapping, actor activity, and tenant grants must still be checked on protected reads.';
COMMENT ON COLUMN identity.browser_sessions.expires_at_ms IS
    'Exclusive positive Unix millisecond session deadline; cannot be extended by updating the row.';

COMMENT ON FUNCTION identity.reject_browser_identity_update() IS
    'Rejects updates to browser login and session evidence while allowing deliberate deletion for single-use consumption, logout, and cleanup.';

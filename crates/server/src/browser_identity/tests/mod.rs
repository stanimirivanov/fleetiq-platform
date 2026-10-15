//! Shared fixtures for observable browser authentication workflows.

mod administration;
mod credential;
mod discovery;
mod external_identities;
mod login;
mod metadata;
mod session;

use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use fleetiq_domain::{
    ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId, UnixTimeMillis,
};
use fleetiq_postgres::{BrowserSession, PostgresBrowserIdentityRepository};
use sqlx::PgPool;
use tower::ServiceExt;

use super::{
    BrowserIdentityConfig, BrowserRequestAuthenticator,
    credential::{LOGIN_COOKIE, SESSION_COOKIE, credential, digest, now, provider_digest},
    provider::{OidcError, OidcIdentity, OidcLogin, OidcProvider},
    routes::postgres_browser_router,
};
use crate::protected_request::RequestAuthenticator;

const PKCE_VERIFIER: &str = "private-pkce-verifier-for-browser-tests-000001";
const NONCE: &str = "private-nonce-for-browser-tests-000001";
const ORIGIN: &str = "https://fleet.example";
const LOGIN_SECRET: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SESSION_SECRET: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[derive(Clone)]
struct TestProvider {
    exchanges: Arc<AtomicUsize>,
}

impl OidcProvider for TestProvider {
    fn begin_login(&self) -> Result<OidcLogin, OidcError> {
        Ok(OidcLogin {
            authorization_url: "https://identity.example/authorize?state=test-state".to_owned(),
            state: "test-state".to_owned(),
            pkce_verifier: PKCE_VERIFIER.to_owned(),
            nonce: NONCE.to_owned(),
        })
    }

    fn finish_login<'a>(
        &'a self,
        code: &'a str,
        verifier: &'a str,
        nonce: &'a str,
    ) -> impl Future<Output = Result<OidcIdentity, OidcError>> + Send + 'a {
        self.exchanges.fetch_add(1, Ordering::SeqCst);
        if code != "valid-code" || verifier != PKCE_VERIFIER || nonce != NONCE {
            return std::future::ready(Err(OidcError::Rejected));
        }
        std::future::ready(Ok(OidcIdentity {
            identity: identity(),
            expires_at: UnixTimeMillis::new(now().expect("test clock").get() + 60_000),
        }))
    }
}

fn config() -> BrowserIdentityConfig {
    BrowserIdentityConfig::from_values(
        ORIGIN,
        "https://identity.example",
        "client",
        "secret",
        "workforce",
    )
    .expect("test configuration")
}

fn router(pool: PgPool, exchanges: Arc<AtomicUsize>) -> axum::Router {
    postgres_browser_router(pool, config(), TestProvider { exchanges })
}

fn identity() -> ExternalIdentity {
    ExternalIdentity::new(
        IdentityAuthorityId::new("workforce").unwrap(),
        ExternalIdentitySubject::new("operator-1").unwrap(),
    )
}

async fn seed_identity(pool: &PgPool) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ('actor-a'), ('actor-b')")
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO identity.external_actor_identities (authority_id, subject, actor_id) VALUES ('workforce', 'operator-1', 'actor-a')")
        .execute(pool).await?;
    Ok(())
}

async fn seed_session(pool: &PgPool, expires_at: UnixTimeMillis) -> Result<(), Box<dyn Error>> {
    PostgresBrowserIdentityRepository::new(pool.clone())
        .insert_session(&BrowserSession {
            credential_digest: digest(SESSION_SECRET),
            provider_digest: provider_digest(&config()),
            identity: identity(),
            actor_id: ActorId::new("actor-a")?,
            expires_at,
        })
        .await?;
    Ok(())
}

fn request(path: &str, cookie: Option<&str>) -> Request<Body> {
    let mut builder = Request::get(path);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    builder.body(Body::empty()).unwrap()
}

fn secret_cookie(name: &str, value: &str) -> String {
    format!("{name}={value}")
}

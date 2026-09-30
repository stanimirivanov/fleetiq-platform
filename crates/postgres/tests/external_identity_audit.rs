//! Integration tests for authorized, cursor-paginated external-identity audit queries.

use std::error::Error;

use fleetiq_application::{
    ExternalIdentityAudit, ExternalIdentityAuditPageSize, ExternalIdentityAuditQuery,
    ExternalIdentityChangeEffect, ExternalIdentityChangeKind, ManageIdentity,
    PlatformAuthorizationService, PlatformBootstrapRequest, PlatformBootstrapService,
};
use fleetiq_domain::{
    ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId, UnixTimeMillis,
};
use fleetiq_postgres::{
    PostgresExternalIdentityAuditRepository, PostgresPlatformAccessPolicy,
    PostgresPlatformBootstrap,
};
use sqlx::PgPool;

mod support;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn pages_are_deterministic_newest_first(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    for (change_id, recorded_at) in [
        ("change-old", 100),
        ("change-a", 200),
        ("change-z", 200),
        ("change-new", 300),
    ] {
        seed_audit(
            &pool,
            change_id,
            "authority-a",
            change_id,
            "attach",
            recorded_at,
            true,
        )
        .await?;
    }

    let audit =
        ExternalIdentityAudit::new(PostgresExternalIdentityAuditRepository::new(pool.clone()));
    let first = audit
        .find_page(manage_identity(&pool).await?, &query(None))
        .await?;
    assert_eq!(ids(&first), ["change-new", "change-z"]);
    let cursor = first.next().expect("older entries should remain").clone();

    let second = audit
        .find_page(manage_identity(&pool).await?, &query(Some(cursor)))
        .await?;
    assert_eq!(ids(&second), ["change-a", "change-old"]);
    assert!(second.next().is_none());
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn reconstructs_complete_meaning_and_redacts_debug_subject(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    seed_audit(
        &pool,
        "change-a",
        "workforce-oidc",
        "sensitive-subject",
        "detach",
        200,
        false,
    )
    .await?;

    let page =
        ExternalIdentityAudit::new(PostgresExternalIdentityAuditRepository::new(pool.clone()))
            .find_page(manage_identity(&pool).await?, &query(None))
            .await?;
    let entry = &page.entries()[0];

    assert_eq!(entry.change_id().as_str(), "change-a");
    assert_eq!(entry.administrator_actor_id().as_str(), "operator-a");
    assert_eq!(entry.target_actor_id().as_str(), "target-a");
    assert_eq!(
        entry.external_identity().authority_id().as_str(),
        "workforce-oidc"
    );
    assert_eq!(
        entry.external_identity().subject().as_str(),
        "sensitive-subject"
    );
    assert_eq!(entry.kind(), ExternalIdentityChangeKind::Detach);
    assert_eq!(entry.recorded_at().get(), 200);
    assert_eq!(entry.effect(), ExternalIdentityChangeEffect::NoStateChange);
    let debug = format!("{entry:?}");
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("sensitive-subject"));
    Ok(())
}

fn query(
    after: Option<fleetiq_application::ExternalIdentityAuditCursor>,
) -> ExternalIdentityAuditQuery {
    ExternalIdentityAuditQuery::new(
        ExternalIdentityAuditPageSize::new(2).expect("valid page size"),
        after,
    )
}

fn ids(page: &fleetiq_application::ExternalIdentityAuditPage) -> Vec<&str> {
    page.entries()
        .iter()
        .map(|entry| entry.change_id().as_str())
        .collect()
}

async fn seed(pool: &PgPool) -> Result<(), Box<dyn Error>> {
    PlatformBootstrapService::new(PostgresPlatformBootstrap::new(pool.clone()))
        .bootstrap(&PlatformBootstrapRequest::new(
            actor_id("operator-a"),
            external_identity("bootstrap-oidc", "operator-subject"),
            UnixTimeMillis::new(1),
        ))
        .await?;
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ('target-a')")
        .execute(pool)
        .await?;
    Ok(())
}

async fn manage_identity(
    pool: &PgPool,
) -> Result<fleetiq_application::AuthorizedPlatform<ManageIdentity>, Box<dyn Error>> {
    Ok(
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
            .authorize::<ManageIdentity>(support::authenticated_actor("operator-a").await)
            .await?,
    )
}

#[allow(clippy::too_many_arguments)]
async fn seed_audit(
    pool: &PgPool,
    change_id: &str,
    authority_id: &str,
    subject: &str,
    kind: &str,
    recorded_at: i64,
    state_changed: bool,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO identity.external_identity_changes \
         (change_id, administrator_actor_id, target_actor_id, authority_id, \
          subject, change_kind, recorded_at_ms, state_changed) \
         VALUES ($1, 'operator-a', 'target-a', $2, $3, $4, $5, $6)",
    )
    .bind(change_id)
    .bind(authority_id)
    .bind(subject)
    .bind(kind)
    .bind(recorded_at)
    .bind(state_changed)
    .execute(pool)
    .await?;
    Ok(())
}

fn external_identity(authority_id: &str, subject: &str) -> ExternalIdentity {
    ExternalIdentity::new(
        IdentityAuthorityId::new(authority_id).expect("valid authority ID"),
        ExternalIdentitySubject::new(subject).expect("valid subject"),
    )
}

fn actor_id(value: &str) -> ActorId {
    ActorId::new(value).expect("valid actor ID")
}

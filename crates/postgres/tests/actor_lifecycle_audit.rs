//! Integration tests for authorized, cursor-paginated actor lifecycle audit queries.

use std::error::Error;

use fleetiq_application::{
    ActorLifecycleAudit, ActorLifecycleAuditPageSize, ActorLifecycleAuditQuery,
    ActorLifecycleChangeEffect, ActorLifecycleChangeKind, ManageIdentity,
    PlatformAuthorizationService, PlatformBootstrapRequest, PlatformBootstrapService,
};
use fleetiq_domain::{
    ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId, UnixTimeMillis,
};
use fleetiq_postgres::{
    PostgresActorLifecycleAuditRepository, PostgresPlatformAccessPolicy, PostgresPlatformBootstrap,
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
        seed_audit(&pool, change_id, "deactivate", recorded_at, true).await?;
    }

    let audit = ActorLifecycleAudit::new(PostgresActorLifecycleAuditRepository::new(pool.clone()));
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
async fn reconstructs_complete_actor_lifecycle_audit_meaning(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    seed_audit(&pool, "change-a", "activate", 200, false).await?;

    let page = ActorLifecycleAudit::new(PostgresActorLifecycleAuditRepository::new(pool.clone()))
        .find_page(manage_identity(&pool).await?, &query(None))
        .await?;
    let entry = &page.entries()[0];

    assert_eq!(entry.change_id().as_str(), "change-a");
    assert_eq!(entry.administrator_actor_id().as_str(), "operator-a");
    assert_eq!(entry.target_actor_id().as_str(), "target-a");
    assert_eq!(entry.kind(), ActorLifecycleChangeKind::Activate);
    assert_eq!(entry.recorded_at().get(), 200);
    assert_eq!(entry.effect(), ActorLifecycleChangeEffect::NoStateChange);
    Ok(())
}

fn query(
    after: Option<fleetiq_application::ActorLifecycleAuditCursor>,
) -> ActorLifecycleAuditQuery {
    ActorLifecycleAuditQuery::new(
        ActorLifecycleAuditPageSize::new(2).expect("valid page size"),
        after,
    )
}

fn ids(page: &fleetiq_application::ActorLifecycleAuditPage) -> Vec<&str> {
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

async fn seed_audit(
    pool: &PgPool,
    change_id: &str,
    kind: &str,
    recorded_at: i64,
    state_changed: bool,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO identity.actor_lifecycle_changes \
         (change_id, administrator_actor_id, target_actor_id, change_kind, \
          recorded_at_ms, state_changed) \
         VALUES ($1, 'operator-a', 'target-a', $2, $3, $4)",
    )
    .bind(change_id)
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

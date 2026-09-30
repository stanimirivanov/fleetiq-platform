//! Integration tests for the authorized, cursor-paginated actor catalogue.

use std::error::Error;

use fleetiq_application::{
    ActorCatalog, ActorCatalogPageSize, ActorCatalogQuery, ActorLifecycleState, ManageIdentity,
    PlatformAuthorizationService, PlatformBootstrapRequest, PlatformBootstrapService,
};
use fleetiq_domain::{
    ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId, UnixTimeMillis,
};
use fleetiq_postgres::{
    PostgresActorCatalogRepository, PostgresPlatformAccessPolicy, PostgresPlatformBootstrap,
};
use sqlx::PgPool;

mod support;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn pages_are_deterministic_in_stable_identity_order(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_operator(&pool).await?;
    seed_actor(&pool, "actor-z", true).await?;
    seed_actor(&pool, "actor-a", true).await?;
    seed_actor(&pool, "actor-m", false).await?;

    let catalog = ActorCatalog::new(PostgresActorCatalogRepository::new(pool.clone()));
    let first = catalog
        .find_page(manage_identity(&pool).await?, &query(None))
        .await?;
    assert_eq!(ids(&first), ["actor-a", "actor-m"]);
    let cursor = first.next().expect("later actors should remain").clone();

    let second = catalog
        .find_page(manage_identity(&pool).await?, &query(Some(cursor)))
        .await?;
    assert_eq!(ids(&second), ["actor-z", "operator-a"]);
    assert!(second.next().is_none());
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn entries_reconstruct_active_and_deactivated_state(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_operator(&pool).await?;
    seed_actor(&pool, "actor-active", true).await?;
    seed_actor(&pool, "actor-deactivated", false).await?;

    let page = ActorCatalog::new(PostgresActorCatalogRepository::new(pool.clone()))
        .find_page(manage_identity(&pool).await?, &query(None))
        .await?;

    assert_eq!(page.entries()[0].actor_id().as_str(), "actor-active");
    assert_eq!(page.entries()[0].state(), ActorLifecycleState::Active);
    assert_eq!(page.entries()[1].actor_id().as_str(), "actor-deactivated");
    assert_eq!(page.entries()[1].state(), ActorLifecycleState::Deactivated);
    Ok(())
}

fn query(after: Option<fleetiq_application::ActorCatalogCursor>) -> ActorCatalogQuery {
    ActorCatalogQuery::new(
        ActorCatalogPageSize::new(2).expect("valid page size"),
        after,
    )
}

fn ids(page: &fleetiq_application::ActorCatalogPage) -> Vec<&str> {
    page.entries()
        .iter()
        .map(|entry| entry.actor_id().as_str())
        .collect()
}

async fn seed_operator(pool: &PgPool) -> Result<(), Box<dyn Error>> {
    PlatformBootstrapService::new(PostgresPlatformBootstrap::new(pool.clone()))
        .bootstrap(&PlatformBootstrapRequest::new(
            actor_id("operator-a"),
            external_identity("bootstrap-oidc", "operator-subject"),
            UnixTimeMillis::new(1),
        ))
        .await?;
    Ok(())
}

async fn seed_actor(pool: &PgPool, actor_id: &str, is_active: bool) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO identity.actors (actor_id, is_active) VALUES ($1, $2)")
        .bind(actor_id)
        .bind(is_active)
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

fn external_identity(authority_id: &str, subject: &str) -> ExternalIdentity {
    ExternalIdentity::new(
        IdentityAuthorityId::new(authority_id).expect("valid authority ID"),
        ExternalIdentitySubject::new(subject).expect("valid subject"),
    )
}

fn actor_id(value: &str) -> ActorId {
    ActorId::new(value).expect("valid actor ID")
}

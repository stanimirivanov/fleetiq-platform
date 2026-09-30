//! Integration tests for authorized current external-identity discovery.

use std::error::Error;

use fleetiq_application::{
    ExternalIdentityCatalog, ExternalIdentityCatalogCursor, ExternalIdentityCatalogPageSize,
    ExternalIdentityCatalogQuery, ManageIdentity, PlatformAuthorizationService,
    PlatformBootstrapRequest, PlatformBootstrapService,
};
use fleetiq_domain::{
    ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId, UnixTimeMillis,
};
use fleetiq_postgres::{
    PostgresExternalIdentityCatalogRepository, PostgresPlatformAccessPolicy,
    PostgresPlatformBootstrap,
};
use sqlx::PgPool;

mod support;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn pages_are_deterministic_and_actor_scoped(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_operator(&pool).await?;
    seed_actor(&pool, "actor-a").await?;
    seed_actor(&pool, "actor-b").await?;
    seed_identity(&pool, "actor-a", "authority-b", "subject-z").await?;
    seed_identity(&pool, "actor-a", "authority-a", "subject-z").await?;
    seed_identity(&pool, "actor-a", "authority-b", "subject-a").await?;
    seed_identity(&pool, "actor-b", "authority-a", "subject-a").await?;

    let catalog =
        ExternalIdentityCatalog::new(PostgresExternalIdentityCatalogRepository::new(pool.clone()));
    let first = catalog
        .find_page(manage_identity(&pool).await?, &query("actor-a", None))
        .await?;
    assert_eq!(
        identities(&first),
        [("authority-a", "subject-z"), ("authority-b", "subject-a")]
    );
    let cursor = first
        .next()
        .expect("one later mapping should remain")
        .clone();

    let second = catalog
        .find_page(
            manage_identity(&pool).await?,
            &query("actor-a", Some(cursor)),
        )
        .await?;
    assert_eq!(identities(&second), [("authority-b", "subject-z")]);
    assert!(second.next().is_none());
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn actor_without_mappings_returns_an_empty_terminal_page(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_operator(&pool).await?;
    seed_actor(&pool, "actor-empty").await?;

    let page =
        ExternalIdentityCatalog::new(PostgresExternalIdentityCatalogRepository::new(pool.clone()))
            .find_page(manage_identity(&pool).await?, &query("actor-empty", None))
            .await?;

    assert!(page.entries().is_empty());
    assert!(page.next().is_none());
    Ok(())
}

fn query(
    actor_id: &str,
    after: Option<ExternalIdentityCatalogCursor>,
) -> ExternalIdentityCatalogQuery {
    ExternalIdentityCatalogQuery::new(
        ActorId::new(actor_id).expect("valid actor ID"),
        ExternalIdentityCatalogPageSize::new(2).expect("valid page size"),
        after,
    )
}

fn identities(page: &fleetiq_application::ExternalIdentityCatalogPage) -> Vec<(&str, &str)> {
    page.entries()
        .iter()
        .map(|entry| {
            let identity = entry.external_identity();
            (
                identity.authority_id().as_str(),
                identity.subject().as_str(),
            )
        })
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

async fn seed_actor(pool: &PgPool, actor_id: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ($1)")
        .bind(actor_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn seed_identity(
    pool: &PgPool,
    actor_id: &str,
    authority_id: &str,
    subject: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO identity.external_actor_identities (authority_id, subject, actor_id) VALUES ($1, $2, $3)",
    )
    .bind(authority_id)
    .bind(subject)
    .bind(actor_id)
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

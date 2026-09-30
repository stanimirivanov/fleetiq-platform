//! Integration tests for authorized current platform-grant discovery.

use std::error::Error;

use fleetiq_application::{
    ManagePlatformAccess, PlatformAction, PlatformAuthorizationService, PlatformBootstrapRequest,
    PlatformBootstrapService, PlatformGrantCatalog, PlatformGrantCatalogCursor,
    PlatformGrantCatalogPageSize, PlatformGrantCatalogQuery,
};
use fleetiq_domain::{
    ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId, UnixTimeMillis,
};
use fleetiq_postgres::{
    PostgresPlatformAccessPolicy, PostgresPlatformBootstrap, PostgresPlatformGrantCatalogRepository,
};
use sqlx::PgPool;

mod support;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn pages_are_deterministic_and_actor_scoped(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_operator(&pool).await?;
    seed_actor(&pool, "actor-a").await?;
    seed_actor(&pool, "actor-b").await?;
    seed_grant(&pool, "actor-a", "tenant.provision").await?;
    seed_grant(&pool, "actor-a", "identity.manage").await?;
    seed_grant(&pool, "actor-b", "platform.access.manage").await?;

    let catalog =
        PlatformGrantCatalog::new(PostgresPlatformGrantCatalogRepository::new(pool.clone()));
    let first = catalog
        .find_page(
            manage_platform_access(&pool).await?,
            &query("actor-a", None, 1),
        )
        .await?;
    assert_eq!(actions(&first), [PlatformAction::ManageIdentity]);
    let cursor = first.next().expect("one later grant should remain");

    let second = catalog
        .find_page(
            manage_platform_access(&pool).await?,
            &query("actor-a", Some(cursor), 1),
        )
        .await?;
    assert_eq!(actions(&second), [PlatformAction::ProvisionTenant]);
    assert!(second.next().is_none());
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn every_current_platform_action_is_reconstructed(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_operator(&pool).await?;

    let page = PlatformGrantCatalog::new(PostgresPlatformGrantCatalogRepository::new(pool.clone()))
        .find_page(
            manage_platform_access(&pool).await?,
            &query("operator-a", None, 100),
        )
        .await?;

    assert_eq!(
        actions(&page),
        [
            PlatformAction::ManageIdentity,
            PlatformAction::ManagePlatformAccess,
            PlatformAction::ManageTenantLifecycle,
            PlatformAction::ProvisionTenant,
        ]
    );
    assert!(page.next().is_none());
    Ok(())
}

fn query(
    actor_id: &str,
    after: Option<PlatformGrantCatalogCursor>,
    page_size: u16,
) -> PlatformGrantCatalogQuery {
    PlatformGrantCatalogQuery::new(
        ActorId::new(actor_id).expect("valid actor ID"),
        PlatformGrantCatalogPageSize::new(page_size).expect("valid page size"),
        after,
    )
}

fn actions(page: &fleetiq_application::PlatformGrantCatalogPage) -> Vec<PlatformAction> {
    page.entries().iter().map(|entry| entry.action()).collect()
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

async fn seed_grant(pool: &PgPool, actor_id: &str, action: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO identity.platform_action_grants (actor_id, action) VALUES ($1, $2)")
        .bind(actor_id)
        .bind(action)
        .execute(pool)
        .await?;
    Ok(())
}

async fn manage_platform_access(
    pool: &PgPool,
) -> Result<fleetiq_application::AuthorizedPlatform<ManagePlatformAccess>, Box<dyn Error>> {
    Ok(
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
            .authorize::<ManagePlatformAccess>(support::authenticated_actor("operator-a").await)
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

//! Integration tests for the authorized, cursor-paginated tenant catalogue.

use std::error::Error;

use fleetiq_application::{
    PlatformAction, PlatformAuthorizationService, PlatformBootstrapRequest,
    PlatformBootstrapService, TenantCatalog, TenantCatalogPageSize, TenantCatalogQuery,
    TenantLifecycleState,
};
use fleetiq_domain::{
    ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId, UnixTimeMillis,
};
use fleetiq_postgres::{
    PostgresPlatformAccessPolicy, PostgresPlatformBootstrap, PostgresTenantCatalogRepository,
};
use sqlx::PgPool;

mod support;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn pages_are_deterministic_in_stable_identity_order(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_operator(&pool).await?;
    seed_tenant(&pool, "tenant-z", true).await?;
    seed_tenant(&pool, "tenant-a", true).await?;
    seed_tenant(&pool, "tenant-m", false).await?;

    let catalog = TenantCatalog::new(PostgresTenantCatalogRepository::new(pool.clone()));
    let first = catalog
        .find_page(manage_tenant_lifecycle(&pool).await?, &query(None))
        .await?;
    assert_eq!(ids(&first), ["tenant-a", "tenant-m"]);
    let cursor = first
        .next()
        .expect("one later tenant should remain")
        .clone();

    let second = catalog
        .find_page(manage_tenant_lifecycle(&pool).await?, &query(Some(cursor)))
        .await?;
    assert_eq!(ids(&second), ["tenant-z"]);
    assert!(second.next().is_none());
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn entries_reconstruct_active_and_suspended_state(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_operator(&pool).await?;
    seed_tenant(&pool, "tenant-active", true).await?;
    seed_tenant(&pool, "tenant-suspended", false).await?;

    let page = TenantCatalog::new(PostgresTenantCatalogRepository::new(pool.clone()))
        .find_page(manage_tenant_lifecycle(&pool).await?, &query(None))
        .await?;

    assert_eq!(page.entries()[0].tenant_id().as_str(), "tenant-active");
    assert_eq!(page.entries()[0].state(), TenantLifecycleState::Active);
    assert_eq!(page.entries()[1].tenant_id().as_str(), "tenant-suspended");
    assert_eq!(page.entries()[1].state(), TenantLifecycleState::Suspended);
    Ok(())
}

fn query(after: Option<fleetiq_application::TenantCatalogCursor>) -> TenantCatalogQuery {
    TenantCatalogQuery::new(
        TenantCatalogPageSize::new(2).expect("valid page size"),
        after,
    )
}

fn ids(page: &fleetiq_application::TenantCatalogPage) -> Vec<&str> {
    page.entries()
        .iter()
        .map(|entry| entry.tenant_id().as_str())
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

async fn seed_tenant(pool: &PgPool, tenant_id: &str, is_active: bool) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id, is_active) VALUES ($1, $2)")
        .bind(tenant_id)
        .bind(is_active)
        .execute(pool)
        .await?;
    Ok(())
}

async fn manage_tenant_lifecycle(
    pool: &PgPool,
) -> Result<fleetiq_application::AuthorizedPlatformAction, Box<dyn Error>> {
    Ok(
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
            .authorize(
                support::authenticated_actor("operator-a").await,
                PlatformAction::ManageTenantLifecycle,
            )
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

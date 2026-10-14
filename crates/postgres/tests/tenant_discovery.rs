//! Real PostgreSQL current-policy discovery, without authorization caching.
use fleetiq_access::{TenantAction, TenantDiscovery, TenantDiscoveryQuery};
use fleetiq_domain::TenantId;
use fleetiq_postgres::PostgresTenantDiscoveryRepository;
use sqlx::PgPool;
use std::error::Error;
mod support;

async fn seed(pool: &PgPool) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO identity.actors (actor_id) VALUES ('actor-a'), ('actor-b'), ('actor-empty')",
    )
    .execute(pool)
    .await?;
    sqlx::query("INSERT INTO metadata.tenants (tenant_id, is_active) VALUES ('tenant-a', true), ('tenant-b', true), ('tenant-c', true), ('tenant-hidden', true), ('tenant-suspended', false)")
        .execute(pool).await?;
    sqlx::query("INSERT INTO identity.tenant_action_grants (tenant_id, actor_id, action) VALUES ('tenant-c','actor-a','access.manage'), ('tenant-a','actor-a','metadata.read'), ('tenant-a','actor-a','metadata.manage'), ('tenant-b','actor-a','metadata.read'), ('tenant-hidden','actor-b','metadata.read'), ('tenant-suspended','actor-a','metadata.read')")
        .execute(pool).await?;
    sqlx::query("INSERT INTO identity.platform_action_grants (actor_id, action) VALUES ('actor-b','identity.manage'), ('actor-empty','tenant.provision')").execute(pool).await?;
    Ok(())
}
fn query(limit: u16, after: Option<&str>) -> TenantDiscoveryQuery {
    TenantDiscoveryQuery::new(limit, after.map(|id| TenantId::new(id).unwrap())).unwrap()
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn pages_contain_complete_exact_actions_and_isolate_actors(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let service = TenantDiscovery::new(PostgresTenantDiscoveryRepository::new(pool.clone()));
    let first = service
        .find_page(
            support::authenticated_actor("actor-a").await,
            &query(1, None),
        )
        .await?;
    assert_eq!(first.tenants().len(), 1);
    assert_eq!(first.tenants()[0].tenant_id().as_str(), "tenant-a");
    assert_eq!(
        first.tenants()[0].actions(),
        [TenantAction::ManageMetadata, TenantAction::ReadMetadata]
    );
    assert_eq!(first.next_after().unwrap().as_str(), "tenant-a");
    let second = service
        .find_page(
            support::authenticated_actor("actor-a").await,
            &query(2, Some("tenant-a")),
        )
        .await?;
    assert_eq!(
        second
            .tenants()
            .iter()
            .map(|entry| entry.tenant_id().as_str())
            .collect::<Vec<_>>(),
        ["tenant-b", "tenant-c"]
    );
    assert_eq!(second.tenants()[1].actions(), [TenantAction::ManageAccess]);
    assert!(second.next_after().is_none());
    let other = service
        .find_page(
            support::authenticated_actor("actor-b").await,
            &query(100, None),
        )
        .await?;
    assert_eq!(other.tenants().len(), 1);
    assert_eq!(other.tenants()[0].tenant_id().as_str(), "tenant-hidden");
    let empty = service
        .find_page(
            support::authenticated_actor("actor-empty").await,
            &query(100, None),
        )
        .await?;
    assert!(
        empty.tenants().is_empty(),
        "platform authority alone grants no tenant visibility"
    );
    assert!(empty.next_after().is_none());
    let absent = service
        .find_page(
            support::authenticated_actor("absent").await,
            &query(100, None),
        )
        .await?;
    assert!(absent.tenants().is_empty());
    assert!(absent.next_after().is_none());
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn revocation_suspension_and_actor_deactivation_affect_fresh_pages(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let service = TenantDiscovery::new(PostgresTenantDiscoveryRepository::new(pool.clone()));
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND actor_id='actor-a' AND action='metadata.manage'").execute(&pool).await?;
    let first = service
        .find_page(
            support::authenticated_actor("actor-a").await,
            &query(1, None),
        )
        .await?;
    assert_eq!(first.tenants()[0].actions(), [TenantAction::ReadMetadata]);
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND actor_id='actor-a'").execute(&pool).await?;
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-b'")
        .execute(&pool)
        .await?;
    // A cursor remains a position after the tenant it named loses access.
    let next = service
        .find_page(
            support::authenticated_actor("actor-a").await,
            &query(1, Some("tenant-a")),
        )
        .await?;
    assert_eq!(next.tenants()[0].tenant_id().as_str(), "tenant-c");
    assert!(next.next_after().is_none());
    sqlx::query("UPDATE identity.actors SET is_active=false WHERE actor_id='actor-a'")
        .execute(&pool)
        .await?;
    let inactive = service
        .find_page(
            support::authenticated_actor("actor-a").await,
            &query(100, None),
        )
        .await?;
    assert!(inactive.tenants().is_empty());
    assert!(inactive.next_after().is_none());
    Ok(())
}

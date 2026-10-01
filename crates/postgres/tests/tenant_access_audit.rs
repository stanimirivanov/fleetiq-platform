//! Integration tests for authorized, cursor-paginated access audit queries.

use std::error::Error;

use fleetiq_access::{ManageAccess, TenantAction, TenantAuthorizationService};
use fleetiq_domain::TenantId;
use fleetiq_identity::{
    TenantAccessAudit, TenantAccessAuditPageSize, TenantAccessAuditQuery, TenantAccessChangeEffect,
    TenantAccessChangeKind,
};
use fleetiq_postgres::{PostgresTenantAccessAuditRepository, PostgresTenantAccessPolicy};
use sqlx::PgPool;

mod support;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn pages_are_deterministic_newest_first_and_tenant_scoped(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_tenant(&pool, "tenant-a").await?;
    seed_tenant(&pool, "tenant-b").await?;
    for actor in ["admin-a", "target-a"] {
        seed_actor(&pool, actor).await?;
    }
    seed_grant(&pool, "tenant-a", "admin-a", "access.manage").await?;

    for (change_id, recorded_at) in [
        ("change-old", 100),
        ("change-a", 200),
        ("change-z", 200),
        ("change-new", 300),
    ] {
        seed_audit(&pool, "tenant-a", change_id, recorded_at).await?;
    }
    seed_audit(&pool, "tenant-b", "change-other-tenant", 400).await?;

    let audit = TenantAccessAudit::new(PostgresTenantAccessAuditRepository::new(pool.clone()));
    let first = audit
        .find_page(manage_access(&pool).await?, &query(None))
        .await?;
    assert_eq!(ids(&first), ["change-new", "change-z"]);
    let cursor = first.next().expect("older entries should remain").clone();

    let second = audit
        .find_page(manage_access(&pool).await?, &query(Some(cursor)))
        .await?;
    assert_eq!(ids(&second), ["change-a", "change-old"]);
    assert!(second.next().is_none());
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn reconstructs_complete_audit_meaning(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_tenant(&pool, "tenant-a").await?;
    for actor in ["admin-a", "target-a"] {
        seed_actor(&pool, actor).await?;
    }
    seed_grant(&pool, "tenant-a", "admin-a", "access.manage").await?;
    seed_audit(&pool, "tenant-a", "change-a", 200).await?;

    let page = TenantAccessAudit::new(PostgresTenantAccessAuditRepository::new(pool.clone()))
        .find_page(manage_access(&pool).await?, &query(None))
        .await?;
    let entry = &page.entries()[0];

    assert_eq!(entry.change_id().as_str(), "change-a");
    assert_eq!(entry.tenant_id().as_str(), "tenant-a");
    assert_eq!(entry.administrator_actor_id().as_str(), "admin-a");
    assert_eq!(entry.target_actor_id().as_str(), "target-a");
    assert_eq!(entry.action(), TenantAction::ReadMetadata);
    assert_eq!(entry.kind(), TenantAccessChangeKind::Grant);
    assert_eq!(entry.recorded_at().get(), 200);
    assert_eq!(entry.effect(), TenantAccessChangeEffect::StateChanged);
    Ok(())
}

fn query(after: Option<fleetiq_identity::TenantAccessAuditCursor>) -> TenantAccessAuditQuery {
    TenantAccessAuditQuery::new(
        TenantAccessAuditPageSize::new(2).expect("valid page size"),
        after,
    )
}

fn ids(page: &fleetiq_identity::TenantAccessAuditPage) -> Vec<&str> {
    page.entries()
        .iter()
        .map(|entry| entry.change_id().as_str())
        .collect()
}

async fn manage_access(
    pool: &PgPool,
) -> Result<fleetiq_access::AuthorizedTenant<ManageAccess>, Box<dyn Error>> {
    Ok(
        TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone()))
            .authorize::<ManageAccess>(
                support::authenticated_actor("admin-a").await,
                TenantId::new("tenant-a")?,
            )
            .await?,
    )
}

async fn seed_tenant(pool: &PgPool, tenant_id: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ($1)")
        .bind(tenant_id)
        .execute(pool)
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

async fn seed_grant(
    pool: &PgPool,
    tenant_id: &str,
    actor_id: &str,
    action: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO identity.tenant_action_grants (tenant_id, actor_id, action) \
         VALUES ($1, $2, $3)",
    )
    .bind(tenant_id)
    .bind(actor_id)
    .bind(action)
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_audit(
    pool: &PgPool,
    tenant_id: &str,
    change_id: &str,
    recorded_at: i64,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO identity.tenant_access_changes \
         (tenant_id, change_id, administrator_actor_id, target_actor_id, action, \
          change_kind, recorded_at_ms, state_changed) \
         VALUES ($1, $2, 'admin-a', 'target-a', 'metadata.read', 'grant', $3, true)",
    )
    .bind(tenant_id)
    .bind(change_id)
    .bind(recorded_at)
    .execute(pool)
    .await?;
    Ok(())
}

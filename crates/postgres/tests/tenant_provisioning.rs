//! Integration tests for atomic, idempotent tenant provisioning.

use std::error::Error;

use fleetiq_access::{PlatformAuthorizationService, ProvisionTenant};
use fleetiq_domain::{ActorId, TenantId, TenantProvisioningId, UnixTimeMillis};
use fleetiq_identity::{TenantProvisioningOutcome, TenantProvisioningService};
use fleetiq_postgres::{
    PostgresPlatformAccessPolicy, PostgresRepositoryError, PostgresTenantProvisioning,
    PostgresTenantProvisioningError,
};
use sqlx::PgPool;

mod support;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn creates_tenant_initial_grants_and_audit_with_exact_retry(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_actor(&pool, "operator-a").await?;
    seed_actor(&pool, "admin-a").await?;
    seed_platform_grant(&pool, "operator-a").await?;
    let service = TenantProvisioningService::new(PostgresTenantProvisioning::new(pool.clone()));
    let request = request("provisioning-a", "tenant-a", "admin-a", 1_000);

    assert_eq!(
        service
            .provision(authorization(&pool, "operator-a").await?, request.clone())
            .await?,
        TenantProvisioningOutcome::Provisioned
    );
    assert_eq!(
        service
            .provision(authorization(&pool, "operator-a").await?, request)
            .await?,
        TenantProvisioningOutcome::AlreadyProvisioned
    );

    assert_eq!(tenant_count(&pool).await?, 1);
    assert_eq!(tenant_grant_count(&pool).await?, 3);
    assert_eq!(provisioning_count(&pool).await?, 1);

    let grants = sqlx::query_scalar::<_, String>(
        "SELECT action FROM identity.tenant_action_grants \
         WHERE tenant_id = $1 AND actor_id = $2 ORDER BY action",
    )
    .bind("tenant-a")
    .bind("admin-a")
    .fetch_all(&pool)
    .await?;
    assert_eq!(
        grants,
        ["access.manage", "metadata.manage", "metadata.read"]
    );

    let audit = sqlx::query!(
        r#"
        SELECT tenant_id, provisioner_actor_id,
               initial_administrator_actor_id, recorded_at_ms
        FROM identity.tenant_provisionings
        WHERE provisioning_id = $1
        "#,
        "provisioning-a",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(audit.tenant_id, "tenant-a");
    assert_eq!(audit.provisioner_actor_id, "operator-a");
    assert_eq!(audit.initial_administrator_actor_id, "admin-a");
    assert_eq!(audit.recorded_at_ms, 1_000);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_exact_commands_have_one_winner(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_actor(&pool, "operator-a").await?;
    seed_actor(&pool, "admin-a").await?;
    seed_platform_grant(&pool, "operator-a").await?;
    let service = TenantProvisioningService::new(PostgresTenantProvisioning::new(pool.clone()));
    let request = request("provisioning-a", "tenant-a", "admin-a", 1_000);
    let first_authorization = authorization(&pool, "operator-a").await?;
    let second_authorization = authorization(&pool, "operator-a").await?;

    let (first, second) = tokio::join!(
        service.provision(first_authorization, request.clone()),
        service.provision(second_authorization, request),
    );
    let outcomes = [first?, second?];

    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == TenantProvisioningOutcome::Provisioned)
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == TenantProvisioningOutcome::AlreadyProvisioned)
            .count(),
        1
    );
    assert_eq!(tenant_count(&pool).await?, 1);
    assert_eq!(tenant_grant_count(&pool).await?, 3);
    assert_eq!(provisioning_count(&pool).await?, 1);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn both_unique_arbiters_reject_different_command_content(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    for actor in ["operator-a", "admin-a", "admin-b"] {
        seed_actor(&pool, actor).await?;
    }
    seed_platform_grant(&pool, "operator-a").await?;
    let service = TenantProvisioningService::new(PostgresTenantProvisioning::new(pool.clone()));
    service
        .provision(
            authorization(&pool, "operator-a").await?,
            request("provisioning-a", "tenant-a", "admin-a", 1_000),
        )
        .await?;

    for conflicting in [
        request("provisioning-a", "tenant-b", "admin-a", 1_000),
        request("provisioning-b", "tenant-a", "admin-a", 1_000),
        request("provisioning-a", "tenant-a", "admin-b", 1_000),
        request("provisioning-a", "tenant-a", "admin-a", 2_000),
    ] {
        let error = service
            .provision(authorization(&pool, "operator-a").await?, conflicting)
            .await
            .expect_err("different immutable command content must collide");
        assert!(matches!(
            error,
            fleetiq_identity::TenantProvisioningError::Repository(
                PostgresTenantProvisioningError::IdentityCollision { .. }
            )
        ));
    }
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn missing_initial_administrator_rolls_back_every_provisioned_record(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_actor(&pool, "operator-a").await?;
    seed_platform_grant(&pool, "operator-a").await?;
    let service = TenantProvisioningService::new(PostgresTenantProvisioning::new(pool.clone()));

    let error = service
        .provision(
            authorization(&pool, "operator-a").await?,
            request("provisioning-a", "tenant-a", "admin-missing", 1_000),
        )
        .await
        .expect_err("missing administrator must reject the complete transaction");
    assert!(matches!(
        error,
        fleetiq_identity::TenantProvisioningError::Repository(
            PostgresTenantProvisioningError::Repository(
                PostgresRepositoryError::InvalidReference { .. }
            )
        )
    ));
    assert_eq!(tenant_count(&pool).await?, 0);
    assert_eq!(tenant_grant_count(&pool).await?, 0);
    assert_eq!(provisioning_count(&pool).await?, 0);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn provisioning_audit_is_immutable(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_actor(&pool, "operator-a").await?;
    seed_actor(&pool, "admin-a").await?;
    seed_platform_grant(&pool, "operator-a").await?;
    TenantProvisioningService::new(PostgresTenantProvisioning::new(pool.clone()))
        .provision(
            authorization(&pool, "operator-a").await?,
            request("provisioning-a", "tenant-a", "admin-a", 1_000),
        )
        .await?;

    for statement in [
        "UPDATE identity.tenant_provisionings SET recorded_at_ms = 2 WHERE provisioning_id = 'provisioning-a'",
        "DELETE FROM identity.tenant_provisionings WHERE provisioning_id = 'provisioning-a'",
    ] {
        let error = sqlx::query(statement)
            .execute(&pool)
            .await
            .expect_err("provisioning audit must reject mutation");
        assert_eq!(
            error
                .as_database_error()
                .and_then(sqlx::error::DatabaseError::code)
                .as_deref(),
            Some("P0001")
        );
    }
    Ok(())
}

async fn authorization(
    pool: &PgPool,
    actor_id: &str,
) -> Result<fleetiq_access::AuthorizedPlatform<ProvisionTenant>, Box<dyn Error>> {
    Ok(
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
            .authorize::<ProvisionTenant>(support::authenticated_actor(actor_id).await)
            .await?,
    )
}

fn request(
    provisioning_id: &str,
    tenant_id: &str,
    administrator_actor_id: &str,
    recorded_at_ms: i64,
) -> fleetiq_identity::TenantProvisioningRequest {
    fleetiq_identity::TenantProvisioningRequest::new(
        TenantProvisioningId::new(provisioning_id).expect("valid provisioning ID"),
        TenantId::new(tenant_id).expect("valid tenant ID"),
        ActorId::new(administrator_actor_id).expect("valid administrator actor ID"),
        UnixTimeMillis::new(recorded_at_ms),
    )
}

async fn seed_actor(pool: &PgPool, actor_id: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ($1)")
        .bind(actor_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn seed_platform_grant(pool: &PgPool, actor_id: &str) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO identity.platform_action_grants (actor_id, action) \
         VALUES ($1, 'tenant.provision')",
    )
    .bind(actor_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn tenant_count(pool: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM metadata.tenants")
        .fetch_one(pool)
        .await
}

async fn tenant_grant_count(pool: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM identity.tenant_action_grants")
        .fetch_one(pool)
        .await
}

async fn provisioning_count(pool: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM identity.tenant_provisionings")
        .fetch_one(pool)
        .await
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn delivery_retry_keeps_first_time_but_strict_time_and_provisioner_still_matter(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_actor(&pool, "operator-a").await?;
    seed_actor(&pool, "operator-b").await?;
    seed_actor(&pool, "admin-a").await?;
    seed_platform_grant(&pool, "operator-a").await?;
    seed_platform_grant(&pool, "operator-b").await?;
    let service = TenantProvisioningService::new(PostgresTenantProvisioning::new(pool.clone()));
    let delivery = |time| {
        fleetiq_identity::TenantProvisioningRequest::for_delivery(
            TenantProvisioningId::new("delivery-a").unwrap(),
            TenantId::new("tenant-a").unwrap(),
            ActorId::new("admin-a").unwrap(),
            UnixTimeMillis::new(time),
        )
    };
    let a = authorization(&pool, "operator-a").await?;
    let b = authorization(&pool, "operator-a").await?;
    let (a, b) = tokio::join!(
        service.provision(a, delivery(1000)),
        service.provision(b, delivery(2000))
    );
    let outcomes = [a?, b?];
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| **o == TenantProvisioningOutcome::Provisioned)
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| **o == TenantProvisioningOutcome::AlreadyProvisioned)
            .count(),
        1
    );
    let time=sqlx::query_scalar::<_,i64>("SELECT recorded_at_ms FROM identity.tenant_provisionings WHERE provisioning_id='delivery-a'").fetch_one(&pool).await?;
    assert!([1000, 2000].contains(&time));
    assert_eq!(
        service
            .provision(authorization(&pool, "operator-a").await?, delivery(3000))
            .await?,
        TenantProvisioningOutcome::AlreadyProvisioned
    );
    assert!(matches!(
        service
            .provision(
                authorization(&pool, "operator-a").await?,
                request("delivery-a", "tenant-a", "admin-a", time + 1)
            )
            .await,
        Err(fleetiq_identity::TenantProvisioningError::Repository(
            PostgresTenantProvisioningError::IdentityCollision { .. }
        ))
    ));
    assert!(matches!(
        service
            .provision(authorization(&pool, "operator-b").await?, delivery(3000))
            .await,
        Err(fleetiq_identity::TenantProvisioningError::Repository(
            PostgresTenantProvisioningError::IdentityCollision { .. }
        ))
    ));
    assert_eq!(tenant_grant_count(&pool).await?, 3);
    assert_eq!(provisioning_count(&pool).await?, 1);
    Ok(())
}

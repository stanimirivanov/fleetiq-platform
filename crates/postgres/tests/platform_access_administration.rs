//! Integration tests for authorized, audited platform grant administration.

use std::error::Error;

use fleetiq_access::{
    ManagePlatformAccess, PlatformAction, PlatformAuthorizationError, PlatformAuthorizationService,
    ProvisionTenant,
};
use fleetiq_domain::{
    ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId,
    PlatformAccessChangeId, UnixTimeMillis,
};
use fleetiq_identity::{
    PlatformAccessAdministration, PlatformAccessAdministrationError, PlatformAccessChangeEffect,
    PlatformAccessChangeKind, PlatformAccessChangeOutcome, PlatformAccessChangeRequest,
    PlatformBootstrapRequest, PlatformBootstrapService,
};
use fleetiq_postgres::{
    PostgresPlatformAccessAdministration, PostgresPlatformAccessAdministrationError,
    PostgresPlatformAccessPolicy, PostgresPlatformBootstrap,
};
use sqlx::PgPool;

mod support;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn grant_revoke_and_exact_retry_are_atomic_and_audited(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let administration =
        PlatformAccessAdministration::new(PostgresPlatformAccessAdministration::new(pool.clone()));
    let grant = request(
        "change-grant",
        "actor-target",
        PlatformAction::ProvisionTenant,
        PlatformAccessChangeKind::Grant,
        1_000,
    );

    assert_eq!(
        administration
            .apply(manage_platform_access(&pool).await?, grant.clone())
            .await?,
        PlatformAccessChangeOutcome::Recorded(PlatformAccessChangeEffect::StateChanged)
    );
    assert_eq!(
        administration
            .apply(manage_platform_access(&pool).await?, grant)
            .await?,
        PlatformAccessChangeOutcome::AlreadyRecorded(PlatformAccessChangeEffect::StateChanged)
    );

    let authorization =
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
            .authorize::<ProvisionTenant>(support::authenticated_actor("actor-target").await)
            .await?;
    assert_eq!(authorization.actor_id().as_str(), "actor-target");

    assert_eq!(
        administration
            .apply(
                manage_platform_access(&pool).await?,
                request(
                    "change-grant-again",
                    "actor-target",
                    PlatformAction::ProvisionTenant,
                    PlatformAccessChangeKind::Grant,
                    1_001,
                ),
            )
            .await?,
        PlatformAccessChangeOutcome::Recorded(PlatformAccessChangeEffect::NoStateChange)
    );
    assert_eq!(
        administration
            .apply(
                manage_platform_access(&pool).await?,
                request(
                    "change-revoke",
                    "actor-target",
                    PlatformAction::ProvisionTenant,
                    PlatformAccessChangeKind::Revoke,
                    1_002,
                ),
            )
            .await?,
        PlatformAccessChangeOutcome::Recorded(PlatformAccessChangeEffect::StateChanged)
    );

    let error = PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
        .authorize::<ProvisionTenant>(support::authenticated_actor("actor-target").await)
        .await
        .expect_err("revocation must affect the next policy decision");
    assert!(matches!(error, PlatformAuthorizationError::Denied));

    let first_audit = sqlx::query!(
        r#"
        SELECT administrator_actor_id, target_actor_id, action, change_kind,
               recorded_at_ms, state_changed
        FROM identity.platform_access_changes
        WHERE change_id = 'change-grant'
        "#,
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(first_audit.administrator_actor_id, "operator-a");
    assert_eq!(first_audit.target_actor_id, "actor-target");
    assert_eq!(first_audit.action, "tenant.provision");
    assert_eq!(first_audit.change_kind, "grant");
    assert_eq!(first_audit.recorded_at_ms, 1_000);
    assert!(first_audit.state_changed);
    assert_eq!(audit_count(&pool).await?, 3);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn reused_change_identity_with_different_content_is_a_collision(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    seed_actor(&pool, "actor-other").await?;
    let administration =
        PlatformAccessAdministration::new(PostgresPlatformAccessAdministration::new(pool.clone()));
    administration
        .apply(
            manage_platform_access(&pool).await?,
            request(
                "change-a",
                "actor-target",
                PlatformAction::ProvisionTenant,
                PlatformAccessChangeKind::Grant,
                1_000,
            ),
        )
        .await?;

    let error = administration
        .apply(
            manage_platform_access(&pool).await?,
            request(
                "change-a",
                "actor-other",
                PlatformAction::ManageIdentity,
                PlatformAccessChangeKind::Grant,
                1_001,
            ),
        )
        .await
        .expect_err("one platform change ID must not name different content");

    assert!(matches!(
        error,
        PlatformAccessAdministrationError::Repository(
            PostgresPlatformAccessAdministrationError::IdentityCollision { ref change_id }
        ) if change_id.as_str() == "change-a"
    ));
    assert_eq!(audit_count(&pool).await?, 1);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_exact_commands_have_one_winner(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let administration =
        PlatformAccessAdministration::new(PostgresPlatformAccessAdministration::new(pool.clone()));
    let request = request(
        "change-a",
        "actor-target",
        PlatformAction::ManageIdentity,
        PlatformAccessChangeKind::Grant,
        1_000,
    );
    let first_authorization = manage_platform_access(&pool).await?;
    let second_authorization = manage_platform_access(&pool).await?;

    let (first, second) = tokio::join!(
        administration.apply(first_authorization, request.clone()),
        administration.apply(second_authorization, request),
    );
    let outcomes = [first?, second?];

    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| {
                **outcome
                    == PlatformAccessChangeOutcome::Recorded(
                        PlatformAccessChangeEffect::StateChanged,
                    )
            })
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| {
                **outcome
                    == PlatformAccessChangeOutcome::AlreadyRecorded(
                        PlatformAccessChangeEffect::StateChanged,
                    )
            })
            .count(),
        1
    );
    assert_eq!(audit_count(&pool).await?, 1);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn final_administrator_is_preserved_but_can_revoke_after_delegation(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let administration =
        PlatformAccessAdministration::new(PostgresPlatformAccessAdministration::new(pool.clone()));

    let error = administration
        .apply(
            manage_platform_access(&pool).await?,
            request(
                "revoke-final-administrator",
                "operator-a",
                PlatformAction::ManagePlatformAccess,
                PlatformAccessChangeKind::Revoke,
                1_000,
            ),
        )
        .await
        .expect_err("the final administrator grant must be preserved");
    assert!(matches!(
        error,
        PlatformAccessAdministrationError::Repository(
            PostgresPlatformAccessAdministrationError::LastPlatformAdministrator {
                ref actor_id
            }
        ) if actor_id.as_str() == "operator-a"
    ));
    assert_eq!(audit_count(&pool).await?, 0);

    administration
        .apply(
            manage_platform_access(&pool).await?,
            request(
                "delegate-administration",
                "actor-target",
                PlatformAction::ManagePlatformAccess,
                PlatformAccessChangeKind::Grant,
                1_001,
            ),
        )
        .await?;
    administration
        .apply(
            manage_platform_access(&pool).await?,
            request(
                "revoke-bootstrap-administrator",
                "operator-a",
                PlatformAction::ManagePlatformAccess,
                PlatformAccessChangeKind::Revoke,
                1_002,
            ),
        )
        .await?;

    let delegated =
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
            .authorize::<ManagePlatformAccess>(support::authenticated_actor("actor-target").await)
            .await?;
    assert_eq!(delegated.actor_id().as_str(), "actor-target");
    assert_eq!(audit_count(&pool).await?, 2);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn failed_mutation_rolls_back_and_audit_is_immutable(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let administration =
        PlatformAccessAdministration::new(PostgresPlatformAccessAdministration::new(pool.clone()));

    let error = administration
        .apply(
            manage_platform_access(&pool).await?,
            request(
                "missing-target",
                "actor-missing",
                PlatformAction::ProvisionTenant,
                PlatformAccessChangeKind::Grant,
                1_000,
            ),
        )
        .await
        .expect_err("a missing target actor must fail atomically");
    assert!(matches!(
        error,
        PlatformAccessAdministrationError::Repository(
            PostgresPlatformAccessAdministrationError::Repository(_)
        )
    ));
    assert_eq!(audit_count(&pool).await?, 0);

    administration
        .apply(
            manage_platform_access(&pool).await?,
            request(
                "immutable-change",
                "actor-target",
                PlatformAction::ProvisionTenant,
                PlatformAccessChangeKind::Grant,
                1_001,
            ),
        )
        .await?;
    for statement in [
        "UPDATE identity.platform_access_changes SET state_changed = false",
        "DELETE FROM identity.platform_access_changes",
    ] {
        let error = sqlx::query(statement)
            .execute(&pool)
            .await
            .expect_err("platform audit evidence must reject mutation");
        assert_constraint(&error, "ck_platform_access_changes_immutable");
    }
    Ok(())
}

async fn seed(pool: &PgPool) -> Result<(), Box<dyn Error>> {
    PlatformBootstrapService::new(PostgresPlatformBootstrap::new(pool.clone()))
        .bootstrap(&PlatformBootstrapRequest::new(
            actor_id("operator-a"),
            external_identity("bootstrap-oidc", "operator-subject"),
            UnixTimeMillis::new(100),
        ))
        .await?;
    seed_actor(pool, "actor-target").await?;
    Ok(())
}

async fn seed_actor(pool: &PgPool, actor_id: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ($1)")
        .bind(actor_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn manage_platform_access(
    pool: &PgPool,
) -> Result<fleetiq_access::AuthorizedPlatform<ManagePlatformAccess>, Box<dyn Error>> {
    Ok(
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
            .authorize::<ManagePlatformAccess>(support::authenticated_actor("operator-a").await)
            .await?,
    )
}

fn request(
    change_id: &str,
    target_actor_id: &str,
    action: PlatformAction,
    kind: PlatformAccessChangeKind,
    recorded_at_ms: i64,
) -> PlatformAccessChangeRequest {
    PlatformAccessChangeRequest::new(
        PlatformAccessChangeId::new(change_id).expect("valid platform change ID"),
        actor_id(target_actor_id),
        action,
        kind,
        UnixTimeMillis::new(recorded_at_ms),
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

async fn audit_count(pool: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM identity.platform_access_changes")
        .fetch_one(pool)
        .await
}

fn assert_constraint(error: &sqlx::Error, expected: &str) {
    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::constraint),
        Some(expected)
    );
}

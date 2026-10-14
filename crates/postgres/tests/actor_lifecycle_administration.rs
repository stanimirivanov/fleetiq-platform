//! Integration tests for authorized, audited actor lifecycle administration.

use std::error::Error;

use fleetiq_access::{
    ActorIdentityResolver, ManageIdentity, PlatformAuthorizationError,
    PlatformAuthorizationService, ProvisionTenant, ReadMetadata, TenantAuthorizationError,
    TenantAuthorizationService,
};
use fleetiq_domain::{
    ActorId, ActorLifecycleChangeId, ExternalIdentity, ExternalIdentitySubject,
    IdentityAuthorityId, TenantId, UnixTimeMillis,
};
use fleetiq_identity::{
    ActorLifecycleAdministration, ActorLifecycleAdministrationError, ActorLifecycleChangeEffect,
    ActorLifecycleChangeKind, ActorLifecycleChangeOutcome, ActorLifecycleChangeRequest,
    PlatformBootstrapRequest, PlatformBootstrapService,
};
use fleetiq_postgres::{
    PostgresActorIdentityResolver, PostgresActorLifecycleAdministration,
    PostgresActorLifecycleAdministrationError, PostgresPlatformAccessPolicy,
    PostgresPlatformBootstrap, PostgresTenantAccessPolicy,
};
use sqlx::PgPool;

mod support;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn deactivate_and_reactivate_control_new_authentication_and_authorization(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let administration =
        ActorLifecycleAdministration::new(PostgresActorLifecycleAdministration::new(pool.clone()));
    let deactivate = request(
        "deactivate-target",
        "actor-target",
        ActorLifecycleChangeKind::Deactivate,
        1_000,
    );

    assert_eq!(
        administration
            .apply(manage_identity(&pool).await?, deactivate.clone())
            .await?,
        ActorLifecycleChangeOutcome::Recorded(ActorLifecycleChangeEffect::StateChanged)
    );
    assert_eq!(
        administration
            .apply(manage_identity(&pool).await?, deactivate)
            .await?,
        ActorLifecycleChangeOutcome::AlreadyRecorded(ActorLifecycleChangeEffect::StateChanged)
    );

    let resolver = PostgresActorIdentityResolver::new(pool.clone());
    assert!(
        resolver
            .resolve(&external_identity("workforce", "target-subject"))
            .await?
            .is_none()
    );
    let denied = PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
        .authorize::<ProvisionTenant>(support::authenticated_actor("actor-target").await)
        .await
        .expect_err("inactive actors must fail current policy evaluation");
    assert!(matches!(denied, PlatformAuthorizationError::Denied));
    let tenant_denied =
        TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone()))
            .authorize::<ReadMetadata>(
                support::authenticated_actor("actor-target").await,
                TenantId::new("tenant-a").expect("valid tenant ID"),
            )
            .await
            .expect_err("inactive actors must fail tenant policy evaluation");
    assert!(matches!(tenant_denied, TenantAuthorizationError::Denied));

    assert_eq!(
        administration
            .apply(
                manage_identity(&pool).await?,
                request(
                    "reactivate-target",
                    "actor-target",
                    ActorLifecycleChangeKind::Activate,
                    1_001,
                ),
            )
            .await?,
        ActorLifecycleChangeOutcome::Recorded(ActorLifecycleChangeEffect::StateChanged)
    );
    assert_eq!(
        resolver
            .resolve(&external_identity("workforce", "target-subject"))
            .await?
            .expect("reactivated identity should resolve")
            .as_str(),
        "actor-target"
    );
    let _authorization =
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
            .authorize::<ProvisionTenant>(support::authenticated_actor("actor-target").await)
            .await?;
    let _tenant_authorization =
        TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone()))
            .authorize::<ReadMetadata>(
                support::authenticated_actor("actor-target").await,
                TenantId::new("tenant-a").expect("valid tenant ID"),
            )
            .await?;
    assert_eq!(audit_count(&pool).await?, 2);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn repeated_requested_state_records_no_state_change(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let outcome =
        ActorLifecycleAdministration::new(PostgresActorLifecycleAdministration::new(pool.clone()))
            .apply(
                manage_identity(&pool).await?,
                request(
                    "already-active",
                    "actor-target",
                    ActorLifecycleChangeKind::Activate,
                    1_000,
                ),
            )
            .await?;

    assert_eq!(
        outcome,
        ActorLifecycleChangeOutcome::Recorded(ActorLifecycleChangeEffect::NoStateChange)
    );
    assert_eq!(audit_count(&pool).await?, 1);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn command_identity_collision_and_missing_actor_roll_back(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let administration =
        ActorLifecycleAdministration::new(PostgresActorLifecycleAdministration::new(pool.clone()));
    administration
        .apply(
            manage_identity(&pool).await?,
            request(
                "lifecycle-a",
                "actor-target",
                ActorLifecycleChangeKind::Deactivate,
                1_000,
            ),
        )
        .await?;

    let collision = administration
        .apply(
            manage_identity(&pool).await?,
            request(
                "lifecycle-a",
                "actor-target",
                ActorLifecycleChangeKind::Activate,
                1_001,
            ),
        )
        .await
        .expect_err("one change ID must not name different content");
    assert!(matches!(
        collision,
        ActorLifecycleAdministrationError::Repository(
            PostgresActorLifecycleAdministrationError::IdentityCollision { ref change_id }
        ) if change_id.as_str() == "lifecycle-a"
    ));

    let missing = administration
        .apply(
            manage_identity(&pool).await?,
            request(
                "missing-target",
                "actor-missing",
                ActorLifecycleChangeKind::Deactivate,
                1_002,
            ),
        )
        .await
        .expect_err("missing actors must fail atomically");
    assert!(matches!(
        missing,
        ActorLifecycleAdministrationError::Repository(
            PostgresActorLifecycleAdministrationError::ActorNotFound { ref actor_id }
        ) if actor_id.as_str() == "actor-missing"
    ));
    assert_eq!(audit_count(&pool).await?, 1);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn final_active_recovery_authorities_are_preserved(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let administration =
        ActorLifecycleAdministration::new(PostgresActorLifecycleAdministration::new(pool.clone()));

    let final_identity_manager = administration
        .apply(
            manage_identity(&pool).await?,
            request(
                "deactivate-final-identity-manager",
                "operator-a",
                ActorLifecycleChangeKind::Deactivate,
                1_000,
            ),
        )
        .await
        .expect_err("the final active identity administrator must be preserved");
    assert!(matches!(
        final_identity_manager,
        ActorLifecycleAdministrationError::Repository(
            PostgresActorLifecycleAdministrationError::LastActiveIdentityAdministrator { .. }
        )
    ));

    sqlx::query(
        "INSERT INTO identity.platform_action_grants (actor_id, action) VALUES ($1, 'identity.manage')",
    )
    .bind("actor-target")
    .execute(&pool)
    .await?;
    let final_platform_manager = administration
        .apply(
            manage_identity(&pool).await?,
            request(
                "deactivate-final-platform-manager",
                "operator-a",
                ActorLifecycleChangeKind::Deactivate,
                1_001,
            ),
        )
        .await
        .expect_err("the final active platform administrator must be preserved");
    assert!(matches!(
        final_platform_manager,
        ActorLifecycleAdministrationError::Repository(
            PostgresActorLifecycleAdministrationError::LastActivePlatformAdministrator { .. }
        )
    ));
    assert_eq!(audit_count(&pool).await?, 0);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_exact_commands_have_one_winner_and_audit_is_immutable(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let administration =
        ActorLifecycleAdministration::new(PostgresActorLifecycleAdministration::new(pool.clone()));
    let change = request(
        "concurrent-change",
        "actor-target",
        ActorLifecycleChangeKind::Deactivate,
        1_000,
    );
    let (first, second) = tokio::join!(
        administration.apply(manage_identity(&pool).await?, change.clone()),
        administration.apply(manage_identity(&pool).await?, change),
    );
    let outcomes = [first?, second?];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ActorLifecycleChangeOutcome::Recorded(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ActorLifecycleChangeOutcome::AlreadyRecorded(_)))
            .count(),
        1
    );

    for statement in [
        "UPDATE identity.actor_lifecycle_changes SET state_changed = false",
        "DELETE FROM identity.actor_lifecycle_changes",
    ] {
        let error = sqlx::query(statement)
            .execute(&pool)
            .await
            .expect_err("actor lifecycle evidence must be immutable");
        assert_constraint(&error, "ck_actor_lifecycle_changes_immutable");
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
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ('actor-target')")
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO identity.external_actor_identities (authority_id, subject, actor_id) VALUES ('workforce', 'target-subject', 'actor-target')",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO identity.platform_action_grants (actor_id, action) VALUES ('actor-target', 'tenant.provision')",
    )
    .execute(pool)
    .await?;
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a')")
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO identity.tenant_action_grants (tenant_id, actor_id, action) VALUES ('tenant-a', 'actor-target', 'metadata.read')",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn manage_identity(
    pool: &PgPool,
) -> Result<fleetiq_access::AuthorizedPlatform<ManageIdentity>, Box<dyn Error>> {
    Ok(
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
            .authorize::<ManageIdentity>(support::authenticated_actor("operator-a").await)
            .await?,
    )
}

fn request(
    change_id: &str,
    target_actor_id: &str,
    kind: ActorLifecycleChangeKind,
    recorded_at_ms: i64,
) -> ActorLifecycleChangeRequest {
    ActorLifecycleChangeRequest::new(
        ActorLifecycleChangeId::new(change_id).expect("valid actor lifecycle change ID"),
        actor_id(target_actor_id),
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
    sqlx::query_scalar("SELECT COUNT(*) FROM identity.actor_lifecycle_changes")
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

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn delivery_retries_retain_first_time_and_effect_while_strict_commands_compare_time(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let service =
        ActorLifecycleAdministration::new(PostgresActorLifecycleAdministration::new(pool.clone()));
    let delivery = |time| {
        ActorLifecycleChangeRequest::for_delivery(
            ActorLifecycleChangeId::new("delivery-a").unwrap(),
            actor_id("actor-target"),
            ActorLifecycleChangeKind::Deactivate,
            UnixTimeMillis::new(time),
        )
    };
    let (first, second) = tokio::join!(
        service.apply(manage_identity(&pool).await?, delivery(1_000)),
        service.apply(manage_identity(&pool).await?, delivery(2_000))
    );
    let outcomes = [first?, second?];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome
                == ActorLifecycleChangeOutcome::Recorded(ActorLifecycleChangeEffect::StateChanged))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome
                == ActorLifecycleChangeOutcome::AlreadyRecorded(
                    ActorLifecycleChangeEffect::StateChanged
                ))
            .count(),
        1
    );
    let stored: i64 = sqlx::query_scalar(
        "SELECT recorded_at_ms FROM identity.actor_lifecycle_changes WHERE change_id='delivery-a'",
    )
    .fetch_one(&pool)
    .await?;
    assert!([1_000, 2_000].contains(&stored));
    assert_eq!(
        service
            .apply(manage_identity(&pool).await?, delivery(3_000))
            .await?,
        ActorLifecycleChangeOutcome::AlreadyRecorded(ActorLifecycleChangeEffect::StateChanged)
    );
    assert!(
        service
            .apply(
                manage_identity(&pool).await?,
                request(
                    "delivery-a",
                    "actor-target",
                    ActorLifecycleChangeKind::Deactivate,
                    3_000
                )
            )
            .await
            .is_err()
    );
    let after: i64 = sqlx::query_scalar(
        "SELECT recorded_at_ms FROM identity.actor_lifecycle_changes WHERE change_id='delivery-a'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(stored, after);
    Ok(())
}

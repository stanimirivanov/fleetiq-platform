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

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn inactive_grant_holder_cannot_replace_final_active_administrator(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    sqlx::query("UPDATE identity.actors SET is_active = false WHERE actor_id = 'actor-target'")
        .execute(&pool)
        .await?;
    let administration =
        PlatformAccessAdministration::new(PostgresPlatformAccessAdministration::new(pool.clone()));
    administration
        .apply(
            manage_platform_access(&pool).await?,
            request(
                "inactive-delegation",
                "actor-target",
                PlatformAction::ManagePlatformAccess,
                PlatformAccessChangeKind::Grant,
                1000,
            ),
        )
        .await?;
    let result = administration
        .apply(
            manage_platform_access(&pool).await?,
            request(
                "unsafe-revoke",
                "operator-a",
                PlatformAction::ManagePlatformAccess,
                PlatformAccessChangeKind::Revoke,
                2000,
            ),
        )
        .await;
    assert!(matches!(
        result,
        Err(PlatformAccessAdministrationError::Repository(
            PostgresPlatformAccessAdministrationError::LastPlatformAdministrator { .. }
        ))
    ));
    assert_eq!(audit_count(&pool).await?, 1);
    assert!(manage_platform_access(&pool).await.is_ok());
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn delivery_retries_preserve_first_time_and_effect_without_reapplying_old_state(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let administration =
        PlatformAccessAdministration::new(PostgresPlatformAccessAdministration::new(pool.clone()));
    let delivery = |time| {
        PlatformAccessChangeRequest::for_delivery(
            PlatformAccessChangeId::new("delivery").unwrap(),
            actor_id("actor-target"),
            PlatformAction::ProvisionTenant,
            PlatformAccessChangeKind::Grant,
            UnixTimeMillis::new(time),
        )
    };
    let (left, right) = tokio::join!(
        administration.apply(manage_platform_access(&pool).await?, delivery(1000)),
        administration.apply(manage_platform_access(&pool).await?, delivery(2000)),
    );
    let outcomes = [left?, right?];
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| matches!(o, PlatformAccessChangeOutcome::Recorded(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| matches!(o, PlatformAccessChangeOutcome::AlreadyRecorded(_)))
            .count(),
        1
    );
    let time: i64 = sqlx::query_scalar(
        "SELECT recorded_at_ms FROM identity.platform_access_changes WHERE change_id='delivery'",
    )
    .fetch_one(&pool)
    .await?;
    assert!([1000, 2000].contains(&time));
    let strict = request(
        "delivery",
        "actor-target",
        PlatformAction::ProvisionTenant,
        PlatformAccessChangeKind::Grant,
        time + 1,
    );
    assert!(matches!(
        administration
            .apply(manage_platform_access(&pool).await?, strict)
            .await,
        Err(PlatformAccessAdministrationError::Repository(
            PostgresPlatformAccessAdministrationError::IdentityCollision { .. }
        ))
    ));
    administration
        .apply(
            manage_platform_access(&pool).await?,
            request(
                "revoke",
                "actor-target",
                PlatformAction::ProvisionTenant,
                PlatformAccessChangeKind::Revoke,
                3000,
            ),
        )
        .await?;
    assert_eq!(
        administration
            .apply(manage_platform_access(&pool).await?, delivery(4000))
            .await?,
        PlatformAccessChangeOutcome::AlreadyRecorded(PlatformAccessChangeEffect::StateChanged)
    );
    let grants: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identity.platform_action_grants WHERE actor_id='actor-target'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(grants, 0);
    assert_eq!(audit_count(&pool).await?, 2);
    let unchanged: i64 = sqlx::query_scalar(
        "SELECT recorded_at_ms FROM identity.platform_access_changes WHERE change_id='delivery'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(unchanged, time);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn lifecycle_and_revocation_cannot_race_away_both_active_administrators(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let grants =
        PlatformAccessAdministration::new(PostgresPlatformAccessAdministration::new(pool.clone()));
    grants
        .apply(
            manage_platform_access(&pool).await?,
            request(
                "delegate-before-race",
                "actor-target",
                PlatformAction::ManagePlatformAccess,
                PlatformAccessChangeKind::Grant,
                1000,
            ),
        )
        .await?;
    let lifecycle = fleetiq_identity::ActorLifecycleAdministration::new(
        fleetiq_postgres::PostgresActorLifecycleAdministration::new(pool.clone()),
    );
    let revoke_authority = manage_platform_access(&pool).await?;
    let lifecycle_authority =
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
            .authorize::<fleetiq_access::ManageIdentity>(
                support::authenticated_actor("operator-a").await,
            )
            .await?;
    let revoke = request(
        "revoke-race",
        "operator-a",
        PlatformAction::ManagePlatformAccess,
        PlatformAccessChangeKind::Revoke,
        2000,
    );
    let deactivate = fleetiq_identity::ActorLifecycleChangeRequest::new(
        fleetiq_domain::ActorLifecycleChangeId::new("deactivate-race")?,
        actor_id("actor-target"),
        fleetiq_identity::ActorLifecycleChangeKind::Deactivate,
        UnixTimeMillis::new(2000),
    );
    let (revoked, deactivated) = tokio::join!(
        grants.apply(revoke_authority, revoke),
        lifecycle.apply(lifecycle_authority, deactivate),
    );
    match (revoked, deactivated) {
        (Ok(PlatformAccessChangeOutcome::Recorded(PlatformAccessChangeEffect::StateChanged)),
            Err(fleetiq_identity::ActorLifecycleAdministrationError::Repository(
                fleetiq_postgres::PostgresActorLifecycleAdministrationError::LastActivePlatformAdministrator { .. }))) |
        (Err(PlatformAccessAdministrationError::Repository(PostgresPlatformAccessAdministrationError::LastPlatformAdministrator { .. })),
            Ok(fleetiq_identity::ActorLifecycleChangeOutcome::Recorded(fleetiq_identity::ActorLifecycleChangeEffect::StateChanged))) => {}
        result => panic!("exactly one change may retire recovery authority: {result:?}"),
    }
    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identity.platform_action_grants AS grants JOIN identity.actors AS actors USING (actor_id) WHERE grants.action='platform.access.manage' AND actors.is_active"
    ).fetch_one(&pool).await?;
    assert_eq!(remaining, 1);
    let lifecycle_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM identity.actor_lifecycle_changes")
            .fetch_one(&pool)
            .await?;
    assert_eq!(audit_count(&pool).await? + lifecycle_count, 2);
    Ok(())
}

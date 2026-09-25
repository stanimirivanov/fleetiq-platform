//! Integration tests for exact external-identity to actor resolution.

use std::error::Error;

use fleetiq_application::ActorIdentityResolver;
use fleetiq_domain::{ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId};
use fleetiq_postgres::PostgresActorIdentityResolver;
use sqlx::PgPool;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn exact_authority_and_subject_resolve_to_the_internal_actor(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_actor(&pool, "actor-a").await?;
    seed_identity(&pool, "workforce", "Subject-A", "actor-a").await?;
    let resolver = PostgresActorIdentityResolver::new(pool);

    let actor = resolver
        .resolve(&identity("workforce", "Subject-A"))
        .await?
        .expect("exact registered identity should resolve");

    assert_eq!(actor.as_str(), "actor-a");
    assert!(
        resolver
            .resolve(&identity("customer", "Subject-A"))
            .await?
            .is_none()
    );
    assert!(
        resolver
            .resolve(&identity("workforce", "subject-a"))
            .await?
            .is_none()
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn one_actor_can_have_multiple_external_identities(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_actor(&pool, "actor-a").await?;
    seed_identity(&pool, "workforce", "employee-42", "actor-a").await?;
    seed_identity(&pool, "automation", "service:fleet-import", "actor-a").await?;
    let resolver = PostgresActorIdentityResolver::new(pool);

    for external in [
        identity("workforce", "employee-42"),
        identity("automation", "service:fleet-import"),
    ] {
        assert_eq!(
            resolver
                .resolve(&external)
                .await?
                .expect("registered identity should resolve")
                .as_str(),
            "actor-a"
        );
    }
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn durable_constraints_prevent_ambiguous_or_invalid_mappings(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_actor(&pool, "actor-a").await?;
    seed_actor(&pool, "actor-b").await?;
    seed_identity(&pool, "workforce", "subject-a", "actor-a").await?;

    let duplicate = seed_identity(&pool, "workforce", "subject-a", "actor-b")
        .await
        .expect_err("one external identity must not map to two actors");
    assert_constraint(&duplicate, "pk_external_actor_identities");

    let missing_actor = seed_identity(&pool, "workforce", "subject-b", "actor-missing")
        .await
        .expect_err("mapping must reference an existing internal actor");
    assert_constraint(&missing_actor, "fk_external_actor_identities_actor");

    let invalid_authority = seed_identity(&pool, " workforce", "subject-b", "actor-a")
        .await
        .expect_err("authority must satisfy the shared identifier invariant");
    assert_constraint(&invalid_authority, "ck_external_actor_identities_authority");

    for invalid_subject in ["", "   ", "subject\nother"] {
        let error = seed_identity(&pool, "workforce", invalid_subject, "actor-a")
            .await
            .expect_err("invalid subject must be rejected by persistence");
        assert_constraint(&error, "ck_external_actor_identities_subject");
    }
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
    authority_id: &str,
    subject: &str,
    actor_id: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO identity.external_actor_identities \
         (authority_id, subject, actor_id) VALUES ($1, $2, $3)",
    )
    .bind(authority_id)
    .bind(subject)
    .bind(actor_id)
    .execute(pool)
    .await?;
    Ok(())
}

fn identity(authority_id: &str, subject: &str) -> ExternalIdentity {
    ExternalIdentity::new(
        IdentityAuthorityId::new(authority_id).expect("test authority ID is valid"),
        ExternalIdentitySubject::new(subject).expect("test subject is valid"),
    )
}

fn assert_constraint(error: &sqlx::Error, expected: &str) {
    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::constraint),
        Some(expected)
    );
}

//! Durable attribution, rollback, missing references and strict compatibility.
use super::super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn gaps_regressions_legacy_and_actor_failures_never_manufacture_history(
    pool: PgPool,
) -> Result {
    seed(&pool).await?;
    for binding in [false, true] {
        assert_eq!(
            append(&pool, &value(binding, 2, 10, "asset-b")).await?,
            RevisionOutcome::Conflict
        );
        assert_eq!(counts(&pool, binding).await?, (0, 0, 0));
        let first = value(binding, 1, 10, "asset-b");
        assert!(matches!(
            PostgresRevisionCommandRepository::new(pool.clone())
                .append(&first, &actor("missing"))
                .await,
            Err(PostgresRepositoryError::InvalidReference { .. })
        ));
        assert_eq!(counts(&pool, binding).await?, (0, 0, 0));
        match &first {
            MetadataRevision::Relationship(v) => {
                PostgresRelationshipRepository::new(pool.clone())
                    .insert_revision(v)
                    .await?;
            }
            MetadataRevision::Binding(v) => {
                PostgresSignalBindingRepository::new(pool.clone())
                    .insert_revision(v)
                    .await?;
            }
        }
        assert_eq!(append(&pool, &first).await?, RevisionOutcome::Conflict);
        assert_eq!(
            append(&pool, &value(binding, 2, 9, "asset-c")).await?,
            RevisionOutcome::Conflict
        );
        assert_eq!(counts(&pool, binding).await?, (1, 1, 0));
        append(&pool, &value(binding, 2, 10, "asset-c")).await?;
        assert_eq!(counts(&pool, binding).await?, (1, 2, 1));
    }
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn missing_references_and_evidence_immutability_are_enforced_by_postgres(
    pool: PgPool,
) -> Result {
    seed(&pool).await?;
    for binding in [false, true] {
        assert!(
            matches!(append(&pool,&value(binding,1,10,"missing")).await,Err(e) if e.downcast_ref::<PostgresRepositoryError>().is_some_and(|e|matches!(e,PostgresRepositoryError::InvalidReference{..})))
        );
        assert_eq!(counts(&pool, binding).await?, (0, 0, 0));
        append(&pool, &value(binding, 1, 10, "asset-b")).await?;
        let statements = if binding {
            [
                "UPDATE metadata.signal_binding_revision_records SET actor_id='operator-b'",
                "DELETE FROM metadata.signal_binding_revision_records",
                "TRUNCATE metadata.signal_binding_revision_records",
            ]
        } else {
            [
                "UPDATE metadata.relationship_revision_records SET actor_id='operator-b'",
                "DELETE FROM metadata.relationship_revision_records",
                "TRUNCATE metadata.relationship_revision_records",
            ]
        };
        for sql in statements {
            let err = sqlx::query(sql).execute(&pool).await.unwrap_err();
            assert_eq!(
                err.as_database_error().unwrap().code().as_deref(),
                Some("23514")
            );
        }
        assert_eq!(counts(&pool, binding).await?, (1, 1, 1));
    }
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn deferred_commit_failure_is_an_error_and_exact_retry_can_then_commit(
    pool: PgPool,
) -> Result {
    seed(&pool).await?;
    sqlx::raw_sql("CREATE FUNCTION metadata.test_reject_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test commit failure'; END $$;").execute(&pool).await?;
    for binding in [false, true] {
        let install = if binding {
            "CREATE CONSTRAINT TRIGGER test_commit_failure AFTER INSERT ON metadata.signal_binding_revision_records DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION metadata.test_reject_commit()"
        } else {
            "CREATE CONSTRAINT TRIGGER test_commit_failure AFTER INSERT ON metadata.relationship_revision_records DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION metadata.test_reject_commit()"
        };
        sqlx::query(install).execute(&pool).await?;
        let first = value(binding, 1, 10, "asset-b");
        assert!(append(&pool, &first).await.is_err());
        assert_eq!(counts(&pool, binding).await?, (0, 0, 0));
        sqlx::query(if binding {
            "DROP TRIGGER test_commit_failure ON metadata.signal_binding_revision_records"
        } else {
            "DROP TRIGGER test_commit_failure ON metadata.relationship_revision_records"
        })
        .execute(&pool)
        .await?;
        assert!(matches!(
            append(&pool, &first).await?,
            RevisionOutcome::Appended(_)
        ));
    }
    Ok(())
}

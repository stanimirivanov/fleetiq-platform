//! Concurrent writers synchronize through database identity locks, never sleeps.
use super::super::*;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn equal_concurrent_appends_have_one_fact_and_original_receipt(pool: PgPool) -> Result {
    seed(&pool).await?;
    for binding in [false, true] {
        for revision in [1, 2] {
            let a = value(
                binding,
                revision,
                100 * i64::try_from(revision).unwrap() + 10,
                "asset-b",
            );
            let b = value(
                binding,
                revision,
                100 * i64::try_from(revision).unwrap() + 20,
                "asset-b",
            );
            let repo = PostgresRevisionCommandRepository::new(pool.clone());
            let actor_a = actor("operator-a");
            let actor_b = actor("operator-b");
            let (a, b) = tokio::join!(repo.append(&a, &actor_a), repo.append(&b, &actor_b));
            let outcomes = [a?, b?];
            let receipts: Vec<_> = outcomes
                .iter()
                .map(|r| match r {
                    RevisionOutcome::Appended(v) | RevisionOutcome::AlreadyAppended(v) => v,
                    RevisionOutcome::Conflict => panic!("equal requests conflicted"),
                })
                .collect();
            assert_eq!(receipts[0], receipts[1]);
            assert_eq!(
                outcomes
                    .iter()
                    .filter(|r| matches!(r, RevisionOutcome::Appended(_)))
                    .count(),
                1
            );
        }
        assert_eq!(counts(&pool, binding).await?, (1, 2, 2));
    }
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn different_concurrent_content_never_overwrites_the_winner(pool: PgPool) -> Result {
    seed(&pool).await?;
    for binding in [false, true] {
        append(&pool, &value(binding, 1, 10, "asset-b")).await?;
        let a = value(binding, 2, 20, "asset-b");
        let b = value(binding, 2, 20, "asset-c");
        let (a, b) = tokio::join!(append(&pool, &a), append(&pool, &b));
        let outcomes = [a?, b?];
        assert_eq!(
            outcomes
                .iter()
                .filter(|r| matches!(r, RevisionOutcome::Appended(_)))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|r| matches!(r, RevisionOutcome::Conflict))
                .count(),
            1
        );
        assert_eq!(counts(&pool, binding).await?, (1, 2, 2));
    }
    Ok(())
}

//! Attributed appends against real PostgreSQL, including immutable exact retries.
mod revision_commands {
    mod concurrency;
    mod invariants;
}
use fleetiq_domain::{
    ActorId, AssetId, AssetRelationship, AssetRelationshipId, DefinitionVersion, DeviceEndpointId,
    EffectiveInterval, PropertyDefinitionId, PropertyDefinitionRef, RecordRevision,
    RecordedRevision, RelationshipEndpoints, RelationshipTypeId, RelationshipTypeRef,
    SignalBinding, SignalBindingId, SignalSource, SignalTarget, SourceSignalId, TenantId,
    UnixTimeMillis,
};
use fleetiq_metadata::{
    MetadataRevision, RelationshipRepository, RelationshipSnapshotQuery, RevisionCommandRepository,
    RevisionOutcome, RevisionReceipt, SignalBindingRepository, SignalBindingSnapshotQuery,
};
use fleetiq_postgres::{
    PostgresRelationshipRepository, PostgresRepositoryError, PostgresRevisionCommandRepository,
    PostgresSignalBindingRepository,
};
use sqlx::PgPool;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
async fn seed(pool: &PgPool) -> Result {
    sqlx::raw_sql(include_str!("revision_commands/topology.sql"))
        .execute(pool)
        .await?;
    Ok(())
}
fn time(v: i64) -> UnixTimeMillis {
    UnixTimeMillis::new(v)
}
fn actor(v: &str) -> ActorId {
    ActorId::new(v).unwrap()
}
fn value(binding: bool, revision: u64, recorded: i64, target: &str) -> MetadataRevision {
    let tenant = TenantId::new("tenant-a").unwrap();
    let record = RecordedRevision::new(RecordRevision::new(revision).unwrap(), time(recorded));
    let interval = EffectiveInterval::new(time(0), Some(time(100))).unwrap();
    if binding {
        MetadataRevision::Binding(SignalBinding::new(
            tenant,
            SignalBindingId::new("fact-a").unwrap(),
            record,
            SignalSource::new(
                AssetId::new("asset-a").unwrap(),
                DeviceEndpointId::new("can").unwrap(),
                SourceSignalId::new("bus.voltage").unwrap(),
            ),
            SignalTarget::new(
                AssetId::new(target).unwrap(),
                PropertyDefinitionRef::new(
                    PropertyDefinitionId::new("voltage").unwrap(),
                    DefinitionVersion::new(1).unwrap(),
                ),
            ),
            interval,
        ))
    } else {
        MetadataRevision::Relationship(AssetRelationship::new(
            tenant,
            AssetRelationshipId::new("fact-a").unwrap(),
            record,
            RelationshipTypeRef::new(
                RelationshipTypeId::new("physical.contains").unwrap(),
                DefinitionVersion::new(1).unwrap(),
            ),
            RelationshipEndpoints::new(
                AssetId::new("asset-a").unwrap(),
                AssetId::new(target).unwrap(),
            ),
            interval,
        ))
    }
}
async fn append(pool: &PgPool, v: &MetadataRevision) -> Result<RevisionOutcome> {
    Ok(PostgresRevisionCommandRepository::new(pool.clone())
        .append(v, &actor("operator-a"))
        .await?)
}
async fn counts(pool: &PgPool, binding: bool) -> Result<(i64, i64, i64)> {
    let query = if binding {
        "SELECT (SELECT count(*) FROM metadata.signal_bindings),(SELECT count(*) FROM metadata.signal_binding_revisions),(SELECT count(*) FROM metadata.signal_binding_revision_records)"
    } else {
        "SELECT (SELECT count(*) FROM metadata.asset_relationships),(SELECT count(*) FROM metadata.asset_relationship_revisions),(SELECT count(*) FROM metadata.relationship_revision_records)"
    };
    Ok(sqlx::query_as(query).fetch_one(pool).await?)
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn corrections_preserve_old_facts_receipts_and_bitemporal_boundaries(pool: PgPool) -> Result {
    seed(&pool).await?;
    for binding in [false, true] {
        let first = value(binding, 1, 10, "asset-b");
        assert_eq!(
            append(&pool, &first).await?,
            RevisionOutcome::Appended(RevisionReceipt::new(actor("operator-a"), time(10)))
        );
        append(&pool, &value(binding, 2, 20, "asset-c")).await?;
        assert_eq!(
            PostgresRevisionCommandRepository::new(pool.clone())
                .append(&value(binding, 1, 999, "asset-b"), &actor("operator-b"))
                .await?,
            RevisionOutcome::AlreadyAppended(RevisionReceipt::new(actor("operator-a"), time(10)))
        );
        assert_eq!(
            append(&pool, &value(binding, 1, 999, "asset-c")).await?,
            RevisionOutcome::Conflict
        );
        assert_eq!(counts(&pool, binding).await?, (1, 2, 2));
        if binding {
            let repo = PostgresSignalBindingRepository::new(pool.clone());
            let MetadataRevision::Binding(original) = first else {
                unreachable!()
            };
            assert_eq!(
                repo.find_revision(original.tenant_id(), original.id(), original.revision())
                    .await?,
                Some(original.clone())
            );
            for (effective, known, target, len) in [
                (0, 9, "", 0),
                (0, 10, "asset-b", 1),
                (99, 20, "asset-c", 1),
                (100, 20, "", 0),
            ] {
                let values = repo
                    .find_for_source(
                        original.tenant_id(),
                        original.source(),
                        SignalBindingSnapshotQuery::new(time(effective), time(known)),
                    )
                    .await?;
                assert_eq!(values.len(), len);
                if len > 0 {
                    assert_eq!(values[0].target().asset_id().as_str(), target);
                }
            }
        } else {
            let repo = PostgresRelationshipRepository::new(pool.clone());
            let MetadataRevision::Relationship(original) = first else {
                unreachable!()
            };
            assert_eq!(
                repo.find_revision(original.tenant_id(), original.id(), original.revision())
                    .await?,
                Some(original.clone())
            );
            for (effective, known, target, len) in [
                (0, 9, "", 0),
                (0, 10, "asset-b", 1),
                (99, 20, "asset-c", 1),
                (100, 20, "", 0),
            ] {
                let values = repo
                    .find_for_asset(
                        original.tenant_id(),
                        original.source_asset_id(),
                        RelationshipSnapshotQuery::new(time(effective), time(known)),
                    )
                    .await?;
                assert_eq!(values.len(), len);
                if len > 0 {
                    assert_eq!(values[0].target_asset_id().as_str(), target);
                }
            }
        }
    }
    Ok(())
}

//! Constant-round-trip fleet snapshot. Hydration is set-based, never per asset.
use super::PostgresCurrentStateRepository;
use crate::{
    error::{PostgresRepositoryError, classify_database_error, corrupt},
    history::HistoryRow,
};
use fleetiq_domain::{AssetId, AssetName, AssetTypeId, AssetTypeRef, DefinitionVersion, TenantId};
use fleetiq_projection::{
    CoordinateEncoding, FleetAsset, FleetQuery, FleetReadRepository, FleetSnapshot, FleetSummary,
};
use std::collections::BTreeMap;
impl FleetReadRepository for PostgresCurrentStateRepository {
    type Error = PostgresRepositoryError;
    /// All aggregates, metadata and both current candidates use the same read-only
    /// repeatable view. Each statement has a 2s deadline; the HTTP use case adds
    /// an 8s whole-operation deadline. Cancellation drops the transaction.
    async fn fleet(
        &self,
        tenant: &TenantId,
        query: &FleetQuery,
    ) -> Result<Option<FleetSnapshot>, Self::Error> {
        let mut tx = self.pool.begin().await.map_err(classify_database_error)?;
        sqlx::raw_sql("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY; SET LOCAL statement_timeout='2s'; SET LOCAL idle_in_transaction_session_timeout='8s';").execute(&mut *tx).await.map_err(classify_database_error)?;
        let m = query.mapping();
        let lat = m.latitude().id().as_str();
        let lv = i64::from(m.latitude().version().get());
        let lon = m.longitude().id().as_str();
        let ov = i64::from(m.longitude().version().get());
        let e7 = m.encoding() == CoordinateEncoding::E7;
        let definitions = sqlx::query_file!(
            "src/projection/fleet/definitions.sql",
            tenant.as_str(),
            lat,
            lv,
            lon,
            ov,
            e7
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        if definitions.found != 2 {
            return Ok(None);
        }
        let head = sqlx::query_file!("src/projection/head.sql", tenant.as_str())
            .fetch_one(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        let counts = sqlx::query_file!(
            "src/projection/fleet/summary.sql",
            tenant.as_str(),
            lat,
            lv,
            lon,
            ov
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        let after = query.after().map(AssetId::as_str);
        let sentinel_limit = i64::from(query.limit()) + 1;
        let mut rows = sqlx::query_file_as!(
            FleetRow,
            "src/projection/fleet/assets.sql",
            tenant.as_str(),
            lat,
            lv,
            lon,
            ov,
            after,
            sentinel_limit
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        let more = rows.len() > usize::from(query.limit());
        rows.truncate(usize::from(query.limit()));
        let ids = rows
            .iter()
            .map(|row| row.asset_id.as_str())
            .collect::<Vec<_>>();
        let mut candidates = if definitions.supported && !ids.is_empty() {
            candidates(&mut tx, tenant, query, &ids).await?
        } else {
            Candidates::default()
        };
        tx.commit().await.map_err(classify_database_error)?;
        let assets = rows
            .into_iter()
            .map(|row| row.into_asset(m, &mut candidates))
            .collect::<Result<Vec<_>, _>>()?;
        let next_after = if more {
            assets.last().map(|a| a.id.clone())
        } else {
            None
        };
        let number = |n| u64::try_from(n).map_err(|e| corrupt("fleet.count", e));
        Ok(Some(FleetSnapshot {
            generation: number(head.generation)?,
            revision: number(head.revision)?,
            pending: head.pending,
            mapping_supported: definitions.supported,
            summary: FleetSummary {
                assets: number(counts.assets)?,
                communication_devices: number(counts.devices)?,
                mapped_assets: number(counts.mapped)?,
                assets_with_current_properties: number(counts.projected)?,
            },
            assets,
            next_after,
        }))
    }
}

#[derive(Default)]
struct Candidates {
    changes: BTreeMap<
        (AssetId, fleetiq_domain::PropertyDefinitionRef),
        fleetiq_history::StoredPropertyChange,
    >,
    ambiguous: std::collections::BTreeSet<String>,
}
/// Bulk source hydration and uniqueness checks are separate from page assembly.
async fn candidates(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant: &TenantId,
    query: &FleetQuery,
    ids: &[&str],
) -> Result<Candidates, PostgresRepositoryError> {
    let m = query.mapping();
    let lat = m.latitude().id().as_str();
    let lv = i64::from(m.latitude().version().get());
    let lon = m.longitude().id().as_str();
    let ov = i64::from(m.longitude().version().get());
    let mut changes = BTreeMap::new();
    let current = sqlx::query_file_as!(
        HistoryRow,
        "src/projection/fleet/current.sql",
        tenant.as_str(),
        ids as &[&str],
        lat,
        lv,
        lon,
        ov
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(classify_database_error)?;
    for row in current {
        let change = row.into_domain(tenant)?;
        changes.insert(
            (
                change.target().asset_id().clone(),
                change.target().property().clone(),
            ),
            change,
        );
    }
    let ambiguous = sqlx::query_file!(
        "src/projection/fleet/ambiguous.sql",
        tenant.as_str(),
        ids as &[&str],
        lat,
        lv,
        lon,
        ov
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(classify_database_error)?
    .into_iter()
    .map(|row| row.asset_id)
    .collect();

    Ok(Candidates { changes, ambiguous })
}

/// Flat checked metadata row, reconstructed separately from consistent SQL reads.
struct FleetRow {
    asset_id: String,
    display_name: String,
    type_id: String,
    type_version: i64,
    mapped: bool,
}
impl FleetRow {
    fn into_asset(
        self,
        m: &fleetiq_projection::LocationMapping,
        candidates: &mut Candidates,
    ) -> Result<FleetAsset, PostgresRepositoryError> {
        let id = AssetId::new(self.asset_id).map_err(|e| corrupt("fleet.asset_id", e))?;
        let latitude = candidates
            .changes
            .remove(&(id.clone(), m.latitude().clone()));
        let longitude = candidates
            .changes
            .remove(&(id.clone(), m.longitude().clone()));
        Ok(FleetAsset {
            name: AssetName::new(self.display_name).map_err(|e| corrupt("fleet.name", e))?,
            asset_type: AssetTypeRef::new(
                AssetTypeId::new(self.type_id).map_err(|e| corrupt("fleet.type", e))?,
                DefinitionVersion::new(
                    u32::try_from(self.type_version).map_err(|e| corrupt("fleet.version", e))?,
                )
                .map_err(|e| corrupt("fleet.version", e))?,
            ),
            mapped: self.mapped,
            ambiguous: candidates.ambiguous.contains(id.as_str()),
            id,
            latitude,
            longitude,
        })
    }
}

//! Bounded exact-identity union; database collation stays identical to hot cursor ordering.
use super::PostgresAssetPropertyChangeView;
use crate::{
    error::{PostgresRepositoryError, classify_database_error, corrupt},
    history::property_change_log::basis_name,
};
use fleetiq_domain::TenantId;
use fleetiq_history::{
    PropertyHistoryCursor, PropertyHistoryPage, PropertyHistoryQuery, PropertyHistoryView,
    RestoredPropertyHistoryView, StoredPropertyChange,
};
use fleetiq_semantics::AssetPropertyChangeId;
use serde_json::{Value, json};
fn key(event: i64, id: &AssetPropertyChangeId, index: usize) -> Value {
    let source = id.observation_id();
    let package = source.decoded_by();
    json!({"event":event,"message":source.message_id().as_str(),"package":package.id().as_str(),"version":package.version().as_str(),"output":source.output_index(),"binding":id.binding_id().as_str(),"revision":id.binding_revision().get().to_string(),"basis":basis_name(id.event_time_basis()),"index":index})
}
impl RestoredPropertyHistoryView for PostgresAssetPropertyChangeView {
    async fn list_with_restored(
        &self,
        tenant: &TenantId,
        query: &PropertyHistoryQuery,
        restored: &[StoredPropertyChange],
    ) -> Result<PropertyHistoryPage, PostgresRepositoryError> {
        if restored.len() > 16 || restored.iter().any(|r| r.tenant_id() != tenant) {
            return Err(corrupt("restored_history", "invalid scope or profile"));
        }
        let hot = self.list_property(tenant, query).await?;
        let mut rows = hot.changes().to_vec();
        for restored in restored.iter().filter(|r| {
            r.target() == query.target()
                && r.event_at() >= query.start()
                && r.event_at() < query.end()
        }) {
            if let Some(existing) = rows.iter().find(|h| h.id() == restored.id()) {
                if existing != restored {
                    return Err(corrupt("restored_history", "immutable evidence differs"));
                }
            } else {
                rows.push(restored.clone());
            }
        }
        let keys = json!(
            rows.iter()
                .enumerate()
                .map(|(i, r)| key(r.event_at().get(), r.id(), i))
                .collect::<Vec<_>>()
        );
        let after = query
            .after()
            .map_or(json!({}), |c| key(c.event_at().get(), c.id(), 0));
        let mut tx = self.pool.begin().await.map_err(classify_database_error)?;
        sqlx::query("SET LOCAL statement_timeout='2s'")
            .execute(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        let indices = sqlx::query_file!(
            "src/history/property_change_view/restored_order.sql",
            keys,
            after
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        tx.commit().await.map_err(classify_database_error)?;
        let more = hot.next_after().is_some() || indices.len() > usize::from(query.limit());
        let selected = indices
            .into_iter()
            .take(usize::from(query.limit()))
            .map(|r| {
                usize::try_from(r.index)
                    .ok()
                    .and_then(|i| rows.get(i).cloned())
                    .ok_or_else(|| corrupt("restored_history", "invalid ordering index"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let next = if more {
            selected
                .last()
                .map(|r| PropertyHistoryCursor::new(r.event_at(), r.id().clone()))
        } else {
            None
        };
        Ok(PropertyHistoryPage::new(selected, next))
    }
}

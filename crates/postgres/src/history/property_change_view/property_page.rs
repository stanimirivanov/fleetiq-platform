//! Set-based target history, with shared source and interpretation reconstruction.
use super::{PostgresAssetPropertyChangeView, row::HistoryRow};
use crate::error::{PostgresRepositoryError, classify_database_error};
use bigdecimal::BigDecimal;
use fleetiq_domain::TenantId;
use fleetiq_history::{
    PropertyHistoryCursor, PropertyHistoryPage, PropertyHistoryQuery, PropertyHistoryView,
};
use fleetiq_semantics::AssetPropertyChangeId;

impl PropertyHistoryView for PostgresAssetPropertyChangeView {
    async fn list_property(
        &self,
        tenant: &TenantId,
        query: &PropertyHistoryQuery,
    ) -> Result<PropertyHistoryPage, PostgresRepositoryError> {
        let cursor = query.after();
        let id = cursor.map(PropertyHistoryCursor::id);
        let source = id.map(AssetPropertyChangeId::observation_id);
        // Empty identifiers, output -1 and revision 0 form a private lower key
        // before every valid interpretation at start. This keeps the range
        // predicate indexable without an optional-cursor OR or OFFSET scan.
        let mut tx = self.pool.begin().await.map_err(classify_database_error)?;
        // One expensive fan-out must not monopolize a pooled database session.
        // SET LOCAL is restored on commit, error, timeout and cancellation.
        sqlx::query("SET LOCAL statement_timeout = '5s'")
            .execute(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        let mut rows = sqlx::query_file_as!(
            HistoryRow,
            "src/history/property_change_view/property_page.sql",
            tenant.as_str(),
            query.target().asset_id().as_str(),
            query.target().property().id().as_str(),
            i64::from(query.target().property().version().get()),
            query.start().get(),
            query.end().get(),
            cursor.map_or(query.start().get(), |c| c.event_at().get()),
            source.map_or("", |s| s.message_id().as_str()),
            source.map_or("", |s| s.decoded_by().id().as_str()),
            source.map_or("", |s| s.decoded_by().version().as_str()),
            source.map_or(-1, |s| i64::from(s.output_index())),
            id.map_or("", |i| i.binding_id().as_str()),
            id.map_or_else(
                || BigDecimal::from(0),
                |i| BigDecimal::from(i.binding_revision().get())
            ),
            id.map_or("", |i| super::basis_name(i.event_time_basis())),
            i64::from(query.limit()) + 1,
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        tx.commit().await.map_err(classify_database_error)?;
        let more = rows.len() > usize::from(query.limit());
        rows.truncate(usize::from(query.limit()));
        let changes = rows
            .into_iter()
            .map(|row| row.into_domain(tenant))
            .collect::<Result<Vec<_>, _>>()?;
        let next = if more {
            changes
                .last()
                .map(|c| PropertyHistoryCursor::new(c.event_at(), c.id().clone()))
        } else {
            None
        };
        Ok(PropertyHistoryPage::new(changes, next))
    }
}

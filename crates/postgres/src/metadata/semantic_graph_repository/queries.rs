use super::PostgresSemanticGraphRead;
use crate::{
    error::{PostgresRepositoryError, classify_database_error},
    metadata::{
        asset_repository::AssetRow, relationship_repository::row::RelationshipRevisionRow,
        signal_binding_repository::row::SignalBindingRevisionRow,
    },
};
use fleetiq_domain::{
    AssetId, AssetRelationship, AssetRelationshipId, SignalBinding, SignalBindingId,
};
use fleetiq_metadata::{AssetCatalogueEntry, SemanticGraphRead};

impl SemanticGraphRead for PostgresSemanticGraphRead {
    async fn assets(&mut self, ids: &[AssetId]) -> Result<Vec<AssetCatalogueEntry>, Self::Error> {
        let ids = ids.iter().map(AssetId::as_str).collect::<Vec<_>>();
        let rows = sqlx::query_file_as!(
            AssetRow,
            "src/metadata/semantic_graph_repository/assets.sql",
            self.tenant.as_str(),
            &ids as &[&str]
        )
        .fetch_all(&mut *self.transaction)
        .await
        .map_err(classify_database_error)?;
        rows.into_iter()
            .map(AssetRow::into_catalogue_entry)
            .collect()
    }
    type Error = PostgresRepositoryError;
    async fn relationships(
        &mut self,
        frontier: &[AssetId],
        excluded: &[AssetRelationshipId],
        limit: u16,
    ) -> Result<Vec<AssetRelationship>, Self::Error> {
        let frontier = frontier.iter().map(AssetId::as_str).collect::<Vec<_>>();
        let excluded = excluded
            .iter()
            .map(AssetRelationshipId::as_str)
            .collect::<Vec<_>>();
        let rows = sqlx::query_file_as!(
            RelationshipRevisionRow,
            "src/metadata/semantic_graph_repository/relationships.sql",
            self.tenant.as_str(),
            &frontier as &[&str],
            self.query.effective_at().get(),
            self.query.known_at().get(),
            &excluded as &[&str],
            i64::from(limit)
        )
        .fetch_all(&mut *self.transaction)
        .await
        .map_err(classify_database_error)?;
        rows.into_iter()
            .map(|row| row.into_domain(self.tenant.clone()))
            .collect()
    }
    async fn bindings(
        &mut self,
        frontier: &[AssetId],
        excluded: &[SignalBindingId],
        limit: u16,
    ) -> Result<Vec<SignalBinding>, Self::Error> {
        let frontier = frontier.iter().map(AssetId::as_str).collect::<Vec<_>>();
        let excluded = excluded
            .iter()
            .map(SignalBindingId::as_str)
            .collect::<Vec<_>>();
        let rows = sqlx::query_file_as!(
            SignalBindingRevisionRow,
            "src/metadata/semantic_graph_repository/bindings.sql",
            self.tenant.as_str(),
            &frontier as &[&str],
            self.query.effective_at().get(),
            self.query.known_at().get(),
            &excluded as &[&str],
            i64::from(limit)
        )
        .fetch_all(&mut *self.transaction)
        .await
        .map_err(classify_database_error)?;
        rows.into_iter()
            .map(|row| row.into_domain(self.tenant.clone()))
            .collect()
    }
}

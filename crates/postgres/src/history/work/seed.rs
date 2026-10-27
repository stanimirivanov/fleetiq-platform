//! Bounded keyset discovery for pre-migration data; one transactional checkpoint.
use super::{PostgresSemanticWorkError, PostgresSemanticWorkRepository};
use crate::error::classify_database_error;
impl PostgresSemanticWorkRepository {
    pub(super) async fn seed_batch(&self) -> Result<(), PostgresSemanticWorkError> {
        // The locked cursor excludes concurrent seeders. The checked query has
        // separate seekable initial/continuing paths, each capped before joins.
        // A failed commit advances neither work nor cursor; the source trigger
        // covers newly inserted keys behind the committed cursor.
        sqlx::query_file!("src/history/work/seed.sql")
            .execute(&self.pool)
            .await
            .map_err(classify_database_error)?;
        Ok(())
    }
}

//! Rebuildable current state; database functions own atomic set-based writes.
mod maintenance;
mod snapshot;
use sqlx::PgPool;
/// PostgreSQL projection adapter; construction does not spawn work or mutate schema.
#[derive(Clone, Debug)]
pub struct PostgresCurrentStateRepository {
    pool: PgPool,
}
impl PostgresCurrentStateRepository {
    /// Uses a pool connected to the exact fully migrated schema.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

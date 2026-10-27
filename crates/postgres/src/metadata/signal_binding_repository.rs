use std::future::Future;

use fleetiq_domain::{
    AssetId, RecordRevision, SignalBinding, SignalBindingId, SignalSource, TenantId,
};
use fleetiq_metadata::{
    SignalBindingRepository, SignalBindingSnapshotPage, SignalBindingSnapshotPageQuery,
    SignalBindingSnapshotQuery,
};
use fleetiq_semantics::SignalBindingView;
use sqlx::PgPool;

use crate::error::PostgresRepositoryError;

mod commands;
mod common;
mod queries;
pub(super) mod row;

/// PostgreSQL implementation of revisioned signal attribution persistence.
#[derive(Clone, Debug)]
pub struct PostgresSignalBindingRepository {
    pool: PgPool,
}

impl PostgresSignalBindingRepository {
    /// Constructs a signal-binding repository from an existing connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl SignalBindingRepository for PostgresSignalBindingRepository {
    type Error = PostgresRepositoryError;

    fn insert_revision(
        &self,
        binding: &SignalBinding,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.append_revision(binding)
    }

    fn find_revision(
        &self,
        tenant_id: &TenantId,
        binding_id: &SignalBindingId,
        revision: RecordRevision,
    ) -> impl Future<Output = Result<Option<SignalBinding>, Self::Error>> + Send {
        self.find_exact_revision(tenant_id, binding_id, revision)
    }

    fn find_for_source(
        &self,
        tenant_id: &TenantId,
        source: &SignalSource,
        query: SignalBindingSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<SignalBinding>, Self::Error>> + Send {
        self.find_source_snapshot(tenant_id, source, query)
    }

    fn find_for_source_page(
        &self,
        tenant_id: &TenantId,
        source: &SignalSource,
        query: &SignalBindingSnapshotPageQuery,
    ) -> impl Future<Output = Result<SignalBindingSnapshotPage, Self::Error>> + Send {
        self.find_source_snapshot_page(tenant_id, source, query)
    }

    fn find_for_target_page(
        &self,
        tenant_id: &TenantId,
        asset_id: &AssetId,
        query: &SignalBindingSnapshotPageQuery,
    ) -> impl Future<Output = Result<Option<SignalBindingSnapshotPage>, Self::Error>> + Send {
        self.find_target_snapshot_page(tenant_id, asset_id, query)
    }
}

impl SignalBindingView for PostgresSignalBindingRepository {
    type Error = PostgresRepositoryError;

    fn find_for_source(
        &self,
        tenant_id: &TenantId,
        source: &SignalSource,
        query: SignalBindingSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<SignalBinding>, Self::Error>> + Send {
        self.find_source_snapshot(tenant_id, source, query)
    }
}

//! Capability-consuming entry points for protected export operations.
use super::{ExportCodec, ExportFailure, ExportKey, ExportRequest, ExportTicket, PostgresExports};
use fleetiq_access::{AuthorizedTenant, ManageOperations, ReadTelemetry};
use fleetiq_history::exports::ExportInfo;
impl PostgresExports {
    /// Creates only within both independently authorized telemetry and operational scopes.
    /// # Errors
    /// Rejects scope mismatch or preserves bounded request failures.
    pub async fn request_authorized<C: ExportCodec>(
        &self,
        read: AuthorizedTenant<ReadTelemetry>,
        manage: AuthorizedTenant<ManageOperations>,
        key: &ExportKey,
        request: &ExportRequest,
        codec: &C,
    ) -> Result<ExportTicket, ExportFailure> {
        if read.actor_id() != manage.actor_id()
            || read.tenant_id() != request.tenant()
            || manage.tenant_id() != request.tenant()
        {
            return Err(ExportFailure::Disabled);
        }
        self.request(key, request, codec).await
    }
    /// Reads only the exact telemetry-authorized tenant's metadata.
    /// # Errors
    /// Preserves redacted storage/reconstruction failures.
    pub async fn describe_authorized(
        &self,
        read: AuthorizedTenant<ReadTelemetry>,
        ticket: ExportTicket,
    ) -> Result<Option<ExportInfo>, ExportFailure> {
        self.describe(read.tenant_id(), ticket).await
    }
    /// Cancels only within both independently authorized scopes; never reopens a selection.
    /// # Errors
    /// Rejects mismatched authority and preserves redacted storage failures.
    pub async fn cancel_authorized(
        &self,
        read: AuthorizedTenant<ReadTelemetry>,
        manage: AuthorizedTenant<ManageOperations>,
        ticket: ExportTicket,
    ) -> Result<(), ExportFailure> {
        if read.tenant_id() != manage.tenant_id() || read.actor_id() != manage.actor_id() {
            return Err(ExportFailure::Disabled);
        }
        self.cancel(read.tenant_id(), ticket).await
    }
}

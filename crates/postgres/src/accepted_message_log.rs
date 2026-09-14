use std::{error::Error, fmt, future::Future};

use fleetiq_application::{
    AcceptedMessageAppendOutcome, AcceptedMessageLog, PendingAcceptedMessage,
};
use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use fleetiq_telemetry::{
    AcceptedMessage, AcceptedMessageId, AcceptedPayload, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};
use sqlx::PgPool;

use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};

/// PostgreSQL implementation of the accepted-message durability port.
#[derive(Clone, Debug)]
pub struct PostgresAcceptedMessageLog {
    pool: PgPool,
}

impl PostgresAcceptedMessageLog {
    /// Constructs an accepted-message log using an existing connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Appends a candidate or resolves an exact idempotent retry.
    ///
    /// The insert is the durability boundary. On primary-key conflict, a second
    /// statement reads the immutable winner with a fresh `READ COMMITTED`
    /// snapshot. Combining the insert and fallback read in one data-modifying
    /// common table expression could return no row after waiting for a
    /// concurrent uncommitted insert because its original snapshot cannot see
    /// the newly committed winner.
    async fn append_message(
        &self,
        candidate: &PendingAcceptedMessage,
    ) -> Result<AcceptedMessageAppendOutcome, PostgresAcceptedMessageLogError> {
        let inserted = sqlx::query_scalar!(
            r#"
            INSERT INTO ingestion.accepted_messages
                (tenant_id, message_id, device_asset_id, protocol_package_id,
                 protocol_package_version, received_at_ms, payload)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (tenant_id, message_id) DO NOTHING
            RETURNING TRUE AS "inserted!"
            "#,
            candidate.tenant_id().as_str(),
            candidate.id().as_str(),
            candidate.device_asset_id().as_str(),
            candidate.protocol_package().id().as_str(),
            candidate.protocol_package().version().as_str(),
            candidate.received_at().get(),
            candidate.payload().as_bytes(),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?;

        if inserted.is_some() {
            return Ok(AcceptedMessageAppendOutcome::Appended(
                candidate.clone().into_accepted_message(),
            ));
        }

        let existing = sqlx::query_as!(
            AcceptedMessageRow,
            r"
            SELECT tenant_id, message_id, device_asset_id, protocol_package_id,
                   protocol_package_version, received_at_ms, payload
            FROM ingestion.accepted_messages
            WHERE tenant_id = $1 AND message_id = $2
            ",
            candidate.tenant_id().as_str(),
            candidate.id().as_str(),
        )
        .fetch_one(&self.pool)
        .await
        .map_err(classify_database_error)?
        .into_domain()?;

        if existing != candidate.clone().into_accepted_message() {
            return Err(PostgresAcceptedMessageLogError::IdentityCollision {
                tenant_id: candidate.tenant_id().clone(),
                message_id: candidate.id().clone(),
            });
        }

        Ok(AcceptedMessageAppendOutcome::AlreadyPresent(existing))
    }
}

impl AcceptedMessageLog for PostgresAcceptedMessageLog {
    type Error = PostgresAcceptedMessageLogError;

    fn append<'a>(
        &'a self,
        candidate: &'a PendingAcceptedMessage,
    ) -> impl Future<Output = Result<AcceptedMessageAppendOutcome, Self::Error>> + Send + 'a {
        self.append_message(candidate)
    }
}

/// Failure while appending or reconstructing an accepted message.
#[derive(Debug)]
pub enum PostgresAcceptedMessageLogError {
    /// The tenant-scoped identity already names different immutable content.
    IdentityCollision {
        /// Tenant that owns the conflicting identity.
        tenant_id: TenantId,
        /// Stable accepted-message identity that was reused.
        message_id: AcceptedMessageId,
    },
    /// PostgreSQL operation or persisted-data reconstruction failed.
    Repository(PostgresRepositoryError),
}

impl fmt::Display for PostgresAcceptedMessageLogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityCollision {
                tenant_id,
                message_id,
            } => write!(
                formatter,
                "accepted-message identity {tenant_id}.{message_id} already names different immutable content"
            ),
            Self::Repository(error) => error.fmt(formatter),
        }
    }
}

impl Error for PostgresAcceptedMessageLogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::IdentityCollision { .. } => None,
            Self::Repository(error) => Some(error),
        }
    }
}

impl From<PostgresRepositoryError> for PostgresAcceptedMessageLogError {
    fn from(error: PostgresRepositoryError) -> Self {
        Self::Repository(error)
    }
}

#[derive(Debug)]
struct AcceptedMessageRow {
    tenant_id: String,
    message_id: String,
    device_asset_id: String,
    protocol_package_id: String,
    protocol_package_version: String,
    received_at_ms: i64,
    payload: Vec<u8>,
}

impl AcceptedMessageRow {
    fn into_domain(self) -> Result<AcceptedMessage, PostgresRepositoryError> {
        Ok(AcceptedMessage::new(
            AcceptedMessageId::new(self.message_id)
                .map_err(|error| corrupt("accepted_messages.message_id", error))?,
            TenantId::new(self.tenant_id)
                .map_err(|error| corrupt("accepted_messages.tenant_id", error))?,
            AssetId::new(self.device_asset_id)
                .map_err(|error| corrupt("accepted_messages.device_asset_id", error))?,
            ProtocolPackageRef::new(
                ProtocolPackageId::new(self.protocol_package_id)
                    .map_err(|error| corrupt("accepted_messages.protocol_package_id", error))?,
                ProtocolPackageVersion::new(self.protocol_package_version).map_err(|error| {
                    corrupt("accepted_messages.protocol_package_version", error)
                })?,
            ),
            UnixTimeMillis::new(self.received_at_ms),
            AcceptedPayload::new(self.payload),
        ))
    }
}

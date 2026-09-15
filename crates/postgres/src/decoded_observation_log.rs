use std::{error::Error, fmt, future::Future};

use fleetiq_application::{
    DecodedObservationAppendOutcome, DecodedObservationBatch, DecodedObservationLog,
};
use fleetiq_domain::TenantId;
use fleetiq_telemetry::{AcceptedMessageId, ProtocolPackageRef};
use sqlx::PgPool;

use crate::error::PostgresRepositoryError;

mod append;
mod row;

/// PostgreSQL implementation of the decoded-observation durability port.
#[derive(Clone, Debug)]
pub struct PostgresDecodedObservationLog {
    pool: PgPool,
}

impl PostgresDecodedObservationLog {
    /// Constructs a decoded-observation log using an existing connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl DecodedObservationLog for PostgresDecodedObservationLog {
    type Error = PostgresDecodedObservationLogError;

    fn append<'a>(
        &'a self,
        batch: &'a DecodedObservationBatch,
    ) -> impl Future<Output = Result<DecodedObservationAppendOutcome, Self::Error>> + Send + 'a
    {
        self.append_batch(batch)
    }
}

/// Failure while appending or reconstructing a decoded-message result.
#[derive(Debug)]
pub enum PostgresDecodedObservationLogError {
    /// No durable accepted message has the candidate's exact immutable content.
    AcceptedMessageMismatch {
        /// Tenant expected to own the accepted message.
        tenant_id: TenantId,
        /// Stable accepted-message identity supplied by the batch.
        message_id: AcceptedMessageId,
    },
    /// The batch identity already names different immutable decoded output.
    IdentityCollision {
        /// Tenant that owns the conflicting identity.
        tenant_id: TenantId,
        /// Stable accepted-message identity that was reused.
        message_id: AcceptedMessageId,
        /// Immutable decoder release participating in the result identity.
        protocol_package: ProtocolPackageRef,
    },
    /// PostgreSQL operation or persisted-data reconstruction failed.
    Repository(PostgresRepositoryError),
}

impl fmt::Display for PostgresDecodedObservationLogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AcceptedMessageMismatch {
                tenant_id,
                message_id,
            } => write!(
                formatter,
                "accepted-message evidence is missing or different for tenant {}, message {}",
                tenant_id.as_str(),
                message_id.as_str()
            ),
            Self::IdentityCollision {
                tenant_id,
                message_id,
                protocol_package,
            } => write!(
                formatter,
                "decoded-result identity collision for tenant {}, message {}, package {}@{}",
                tenant_id.as_str(),
                message_id.as_str(),
                protocol_package.id().as_str(),
                protocol_package.version().as_str()
            ),
            Self::Repository(error) => error.fmt(formatter),
        }
    }
}

impl Error for PostgresDecodedObservationLogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::AcceptedMessageMismatch { .. } | Self::IdentityCollision { .. } => None,
            Self::Repository(error) => Some(error),
        }
    }
}

impl From<PostgresRepositoryError> for PostgresDecodedObservationLogError {
    fn from(error: PostgresRepositoryError) -> Self {
        Self::Repository(error)
    }
}

//! Attributed immutable corrections share one command boundary and retry contract.
use crate::{AuthorizedTenant, ManageMetadata};
use fleetiq_domain::{
    ActorId, AssetRelationship, RecordRevision, SignalBinding, TenantId, UnixTimeMillis,
};
use std::{error::Error, fmt, future::Future};

/// Complete revision content, kept separate from transport and attribution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetadataRevision {
    /// One directed relationship fact, including its explicit revision.
    Relationship(AssetRelationship),
    /// One source-to-property attribution fact.
    Binding(SignalBinding),
}
impl MetadataRevision {
    /// Returns the owner that must match consumed authorization.
    pub fn tenant_id(&self) -> &TenantId {
        match self {
            Self::Relationship(v) => v.tenant_id(),
            Self::Binding(v) => v.tenant_id(),
        }
    }
    /// Returns the stable relationship or binding identity within its own namespace.
    pub fn id(&self) -> &str {
        match self {
            Self::Relationship(v) => v.id().as_str(),
            Self::Binding(v) => v.id().as_str(),
        }
    }
    /// Returns the exact positive revision requested, never an implicit latest.
    pub fn revision(&self) -> RecordRevision {
        match self {
            Self::Relationship(v) => v.revision(),
            Self::Binding(v) => v.revision(),
        }
    }
    /// Returns host receipt time; retries use the original persisted time instead.
    pub fn recorded_at(&self) -> UnixTimeMillis {
        match self {
            Self::Relationship(v) => v.recorded_at(),
            Self::Binding(v) => v.recorded_at(),
        }
    }
    /// Compares all client-selectable content, excluding trusted receipt time.
    ///
    /// A retry can be observed at another instant by another authorized actor.
    /// Neither event changes the original immutable fact or its attribution.
    pub fn same_content(&self, other: &Self) -> bool {
        if self.tenant_id() != other.tenant_id()
            || self.id() != other.id()
            || self.revision() != other.revision()
        {
            return false;
        }
        match (self, other) {
            (Self::Relationship(a), Self::Relationship(b)) => {
                a.relationship_type() == b.relationship_type()
                    && a.source_asset_id() == b.source_asset_id()
                    && a.target_asset_id() == b.target_asset_id()
                    && a.effective_interval() == b.effective_interval()
            }
            (Self::Binding(a), Self::Binding(b)) => {
                a.source() == b.source()
                    && a.target() == b.target()
                    && a.effective_interval() == b.effective_interval()
            }
            _ => false,
        }
    }
}

/// Original attribution for a durable exact revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevisionReceipt {
    actor_id: ActorId,
    recorded_at: UnixTimeMillis,
}
impl RevisionReceipt {
    /// Reconstructs trusted evidence from the committed fact and actor record.
    pub const fn new(actor_id: ActorId, recorded_at: UnixTimeMillis) -> Self {
        Self {
            actor_id,
            recorded_at,
        }
    }
    /// Returns the first authorized writer, never the retrying actor.
    pub const fn actor_id(&self) -> &ActorId {
        &self.actor_id
    }
    /// Returns the recorded time stored on the immutable revision.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at
    }
}

/// Resolved append outcome; no variant permits overwriting a recorded fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RevisionOutcome {
    /// Fact, stable head and actor attribution committed together.
    Appended(RevisionReceipt),
    /// Identical content already exists with original attribution.
    AlreadyAppended(RevisionReceipt),
    /// Different content, legacy unattributed fact, gap or temporal regression.
    Conflict,
}

/// Consumer-owned port for attributed relationship and binding revision commands.
pub trait RevisionCommandRepository: Send + Sync {
    /// Storage failure, distinct from a resolved revision conflict.
    type Error: Error + Send + Sync + 'static;
    /// Appends a complete fact or resolves an identical attributed retry.
    ///
    /// # Errors
    ///
    /// Returns reference, reconstruction or infrastructure failures. A commit
    /// error is ambiguous and cannot be advertised as successful.
    fn append<'a>(
        &'a self,
        revision: &'a MetadataRevision,
        actor: &'a ActorId,
    ) -> impl Future<Output = Result<RevisionOutcome, Self::Error>> + Send + 'a;
}

/// Consumes exact tenant management evidence before invoking the outbound port.
#[derive(Clone, Debug)]
pub struct RevisionCommandService<R> {
    repository: R,
}
impl<R> RevisionCommandService<R> {
    /// Selects the attributed revision adapter.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}
impl<R: RevisionCommandRepository> RevisionCommandService<R> {
    /// Appends within the authorized tenant using the authenticated actor.
    ///
    /// # Errors
    ///
    /// Rejects foreign content before persistence and preserves storage causes.
    pub async fn append(
        &self,
        authorization: AuthorizedTenant<ManageMetadata>,
        revision: &MetadataRevision,
    ) -> Result<RevisionOutcome, RevisionCommandError<R::Error>> {
        if authorization.tenant_id() != revision.tenant_id() {
            return Err(RevisionCommandError::TenantMismatch);
        }
        self.repository
            .append(revision, authorization.actor_id())
            .await
            .map_err(RevisionCommandError::Repository)
    }
}
/// Failure to execute an authorized attributed append.
#[derive(Debug)]
pub enum RevisionCommandError<E> {
    /// Content does not belong to the consumed tenant scope.
    TenantMismatch,
    /// Storage or reconstruction failed.
    Repository(E),
}
impl<E> fmt::Display for RevisionCommandError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TenantMismatch => "revision is outside the authorized tenant",
            Self::Repository(_) => "metadata revision append failed",
        })
    }
}
impl<E: Error + 'static> Error for RevisionCommandError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(e) => Some(e),
            Self::TenantMismatch => None,
        }
    }
}
#[cfg(test)]
mod tests;

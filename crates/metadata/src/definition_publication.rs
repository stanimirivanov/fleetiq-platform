//! Content-addressed-by-key publication; strict insert callers retain their contract.
use crate::{AuthorizedTenant, ManageMetadata, SemanticDefinition};
use fleetiq_domain::TenantId;
use std::{error::Error, fmt, future::Future};

/// Durable resolution of a definition publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefinitionPublicationOutcome {
    /// A previously absent immutable version was committed.
    Published,
    /// The exact complete content already exists under this tenant/key/version.
    AlreadyPublished,
    /// The version is occupied by different immutable content.
    Conflict,
}

/// Persistence boundary for atomic, content-checked definition publication.
pub trait DefinitionPublicationRepository: Send + Sync {
    /// Storage or reconstruction failure, distinct from a semantic collision.
    type Error: Error + Send + Sync + 'static;

    /// Commits the complete aggregate or resolves the existing immutable version.
    ///
    /// Compare every field and ordered property reference on a retry. Conflicts
    /// never modify stored content. A failed reference must not leave a parent.
    ///
    /// # Errors
    ///
    /// Returns storage failure when publication cannot be durably resolved.
    fn publish<'a>(
        &'a self,
        tenant: &'a TenantId,
        definition: &'a SemanticDefinition,
    ) -> impl Future<Output = Result<DefinitionPublicationOutcome, Self::Error>> + Send + 'a;
}

/// Publishes definitions using exactly one consumed metadata-management grant.
#[derive(Clone, Debug)]
pub struct DefinitionPublicationService<R> {
    repository: R,
}
impl<R> DefinitionPublicationService<R> {
    /// Selects the outward persistence adapter.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}
impl<R: DefinitionPublicationRepository> DefinitionPublicationService<R> {
    /// Derives tenant ownership exclusively from current authorization evidence.
    ///
    /// The immutable key and complete content identify a retry. Publication is
    /// independent of publisher identity and does not claim an actor/time audit.
    ///
    /// # Errors
    ///
    /// Wraps storage failures; an occupied different version is a conflict outcome.
    pub async fn publish(
        &self,
        authorization: AuthorizedTenant<ManageMetadata>,
        definition: &SemanticDefinition,
    ) -> Result<DefinitionPublicationOutcome, DefinitionPublicationError<R::Error>> {
        self.repository
            .publish(authorization.tenant_id(), definition)
            .await
            .map_err(DefinitionPublicationError::Repository)
    }
}

/// Failure to resolve an authorized publication.
#[derive(Debug)]
pub enum DefinitionPublicationError<E> {
    /// Persistence or reconstruction failed.
    Repository(E),
}
impl<E> fmt::Display for DefinitionPublicationError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("definition publication failed")
    }
}
impl<E: Error + 'static> Error for DefinitionPublicationError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(e) => Some(e),
        }
    }
}

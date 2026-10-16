//! Authorized creation of assets and their communication capability.
use crate::{AuthorizedTenant, ManageMetadata};
use fleetiq_domain::{ActorId, Asset, AssetId, CommunicationDevice, TenantId, UnixTimeMillis};
use std::{error::Error, fmt, future::Future};

/// Complete creation content; external identity is never internal asset identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetadataRegistration {
    /// Creates an asset and all of its typed external identifiers.
    Asset(Asset),
    /// Attaches communication capability to an existing asset.
    Device(CommunicationDevice),
}
impl MetadataRegistration {
    /// Returns the owner of the aggregate being registered.
    pub fn tenant_id(&self) -> &TenantId {
        match self {
            Self::Asset(value) => value.tenant_id(),
            Self::Device(value) => value.tenant_id(),
        }
    }
    /// Returns the internal asset identity shared by both registration kinds.
    pub fn asset_id(&self) -> &AssetId {
        match self {
            Self::Asset(value) => value.id(),
            Self::Device(value) => value.asset_id(),
        }
    }
    /// Compares creation content, treating external identifiers as a set.
    ///
    /// Endpoint order remains significant. Registration never edits an existing
    /// object: a later change to its creation content is a collision on retry.
    pub fn same_content(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Asset(left), Self::Asset(right)) => {
                let mut left_ids: Vec<_> = left.external_identifiers().iter().collect();
                let mut right_ids: Vec<_> = right.external_identifiers().iter().collect();
                left_ids.sort_unstable();
                right_ids.sort_unstable();
                left.tenant_id() == right.tenant_id()
                    && left.id() == right.id()
                    && left.asset_type() == right.asset_type()
                    && left.name() == right.name()
                    && left_ids == right_ids
            }
            (Self::Device(left), Self::Device(right)) => left == right,
            _ => false,
        }
    }
}

/// Immutable evidence from the first successful attributed registration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistrationReceipt {
    actor_id: ActorId,
    recorded_at: UnixTimeMillis,
}
impl RegistrationReceipt {
    /// Reconstructs trusted first-registration evidence.
    pub const fn new(actor_id: ActorId, recorded_at: UnixTimeMillis) -> Self {
        Self {
            actor_id,
            recorded_at,
        }
    }
    /// Returns the original authorized registering actor, including on retries.
    pub const fn actor_id(&self) -> &ActorId {
        &self.actor_id
    }
    /// Returns the host receipt time of the original successful registration.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at
    }
}

/// Resolved creation result; collisions never overwrite an existing aggregate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistrationOutcome {
    /// Aggregate, children and evidence were committed together.
    Registered(RegistrationReceipt),
    /// Exact content and original evidence already exist.
    AlreadyRegistered(RegistrationReceipt),
    /// Identity is occupied by other content or lacks attributed evidence.
    Conflict,
}

/// Consumer-owned boundary for atomic, attributed metadata registration.
pub trait RegistrationRepository: Send + Sync {
    /// Storage failure, distinct from a resolved identity collision.
    type Error: Error + Send + Sync + 'static;
    /// Registers complete content and evidence or resolves a committed winner.
    ///
    /// External identifier order is irrelevant; endpoint order is preserved.
    /// Recover only known creation conflicts in a fresh database snapshot.
    /// Never fabricate evidence for objects created by older strict insert paths.
    ///
    /// # Errors
    ///
    /// Returns reference, invariant, reconstruction or infrastructure failures.
    /// A lost commit response is ambiguous: retry the same complete content.
    fn register<'a>(
        &'a self,
        registration: &'a MetadataRegistration,
        actor_id: &'a ActorId,
        received_at: UnixTimeMillis,
    ) -> impl Future<Output = Result<RegistrationOutcome, Self::Error>> + Send + 'a;
}

/// Coordinates creation using one consumed, exact metadata-management grant.
#[derive(Clone, Debug)]
pub struct RegistrationService<R> {
    repository: R,
}
impl<R> RegistrationService<R> {
    /// Selects the outward registration adapter.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}
impl<R: RegistrationRepository> RegistrationService<R> {
    /// Registers content for the authorized tenant with trusted actor attribution.
    ///
    /// The adapter supplies host receipt time. Client JSON cannot choose actor,
    /// tenant or recorded time. A different authorized retrying actor receives
    /// the original evidence; retries do not reattribute the creation.
    ///
    /// # Errors
    ///
    /// Rejects a foreign aggregate before persistence and preserves storage causes.
    pub async fn register(
        &self,
        authorization: AuthorizedTenant<ManageMetadata>,
        registration: &MetadataRegistration,
        received_at: UnixTimeMillis,
    ) -> Result<RegistrationOutcome, RegistrationError<R::Error>> {
        if authorization.tenant_id() != registration.tenant_id() {
            return Err(RegistrationError::TenantMismatch);
        }
        self.repository
            .register(registration, authorization.actor_id(), received_at)
            .await
            .map_err(RegistrationError::Repository)
    }
}

/// Failure to execute an authorized creation.
#[derive(Debug)]
pub enum RegistrationError<E> {
    /// The aggregate does not belong to the tenant in authorization evidence.
    TenantMismatch,
    /// Persistence or reconstruction failed.
    Repository(E),
}
impl<E> fmt::Display for RegistrationError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TenantMismatch => "registration is outside the authorized tenant",
            Self::Repository(_) => "metadata registration failed",
        })
    }
}
impl<E: Error + 'static> Error for RegistrationError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(e) => Some(e),
            Self::TenantMismatch => None,
        }
    }
}
#[cfg(test)]
mod tests;

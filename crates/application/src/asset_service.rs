use std::{error::Error, fmt};

use fleetiq_domain::{Asset, AssetId, TenantId};

use crate::{AssetRepository, AuthorizedTenantAction, TenantAction};

/// Coordinates authorized operations on tenant-owned assets.
///
/// The service consumes authorization evidence for every operation. Reads
/// derive tenant scope from that evidence, while writes additionally verify
/// that the supplied aggregate belongs to the authorized tenant before
/// invoking persistence.
#[derive(Clone, Debug)]
pub struct AssetApplicationService<R> {
    repository: R,
}

impl<R> AssetApplicationService<R> {
    /// Creates the service using the supplied asset repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> AssetApplicationService<R>
where
    R: AssetRepository,
{
    /// Registers one asset using exact `ManageMetadata` authorization.
    ///
    /// Authorization evidence is consumed so it cannot be reused for another
    /// operation. An asset owned by another tenant is rejected before the
    /// repository is called.
    ///
    /// # Errors
    ///
    /// Returns [`AssetApplicationError::ActionRequired`] when the evidence
    /// authorizes another action, [`AssetApplicationError::TenantMismatch`]
    /// when the asset is outside the authorized tenant, or wraps the repository
    /// error when persistence fails.
    pub async fn register(
        &self,
        authorization: AuthorizedTenantAction,
        asset: &Asset,
    ) -> Result<(), AssetApplicationError<R::Error>> {
        require_action(&authorization, TenantAction::ManageMetadata)?;

        if authorization.tenant_id() != asset.tenant_id() {
            return Err(AssetApplicationError::TenantMismatch {
                authorized_tenant_id: authorization.tenant_id().clone(),
                asset_tenant_id: asset.tenant_id().clone(),
            });
        }

        self.repository
            .insert(asset)
            .await
            .map_err(AssetApplicationError::Repository)
    }

    /// Finds an asset using exact `ReadMetadata` authorization.
    ///
    /// Tenant scope is derived exclusively from the consumed authorization
    /// evidence. The caller supplies only the asset identity and cannot select
    /// a different tenant for the repository lookup.
    ///
    /// # Errors
    ///
    /// Returns [`AssetApplicationError::ActionRequired`] when the evidence
    /// authorizes another action, or wraps the repository error when the lookup
    /// fails.
    pub async fn find_by_id(
        &self,
        authorization: AuthorizedTenantAction,
        asset_id: &AssetId,
    ) -> Result<Option<Asset>, AssetApplicationError<R::Error>> {
        require_action(&authorization, TenantAction::ReadMetadata)?;

        self.repository
            .find_by_id(authorization.tenant_id(), asset_id)
            .await
            .map_err(AssetApplicationError::Repository)
    }
}

/// Failure to execute an authorized asset operation.
#[derive(Debug)]
pub enum AssetApplicationError<E> {
    /// The supplied evidence authorizes a different tenant action.
    ActionRequired {
        /// Action required by the attempted operation.
        required: TenantAction,
        /// Action carried by the rejected authorization evidence.
        actual: TenantAction,
    },
    /// The asset aggregate belongs to a tenant other than the authorized one.
    TenantMismatch {
        /// Tenant carried by the authorization evidence.
        authorized_tenant_id: TenantId,
        /// Tenant that owns the rejected asset aggregate.
        asset_tenant_id: TenantId,
    },
    /// The persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for AssetApplicationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ActionRequired { .. } => {
                formatter.write_str("asset operation requires different authorization")
            }
            Self::TenantMismatch { .. } => {
                formatter.write_str("asset is outside the authorized tenant")
            }
            Self::Repository(_) => formatter.write_str("asset repository operation failed"),
        }
    }
}

impl<E> Error for AssetApplicationError<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(error) => Some(error),
            Self::ActionRequired { .. } | Self::TenantMismatch { .. } => None,
        }
    }
}

fn require_action<E>(
    authorization: &AuthorizedTenantAction,
    required: TenantAction,
) -> Result<(), AssetApplicationError<E>> {
    let actual = authorization.action();
    if actual == required {
        Ok(())
    } else {
        Err(AssetApplicationError::ActionRequired { required, actual })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        future::{Future, ready},
        sync::{Arc, Mutex},
    };

    use fleetiq_domain::{ActorId, AssetName, AssetTypeId, AssetTypeRef, DefinitionVersion};

    use crate::{
        AccessDecision, TenantAccessPolicy, TenantAccessRequest, TenantAuthorizationService,
    };

    use super::*;

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum RepositoryCall {
        Insert(Asset),
        FindById(TenantId, AssetId),
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct TestRepositoryError;

    impl fmt::Display for TestRepositoryError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("test repository failed")
        }
    }

    impl Error for TestRepositoryError {}

    #[derive(Clone, Debug)]
    struct RecordingAssetRepository {
        calls: Arc<Mutex<Vec<RepositoryCall>>>,
        insert_result: Result<(), TestRepositoryError>,
        find_result: Result<Option<Asset>, TestRepositoryError>,
    }

    impl Default for RecordingAssetRepository {
        fn default() -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                insert_result: Ok(()),
                find_result: Ok(None),
            }
        }
    }

    impl RecordingAssetRepository {
        fn returning(asset: Asset) -> Self {
            Self {
                find_result: Ok(Some(asset)),
                ..Self::default()
            }
        }

        fn failing() -> Self {
            Self {
                insert_result: Err(TestRepositoryError),
                find_result: Err(TestRepositoryError),
                ..Self::default()
            }
        }

        fn calls(&self) -> Vec<RepositoryCall> {
            self.calls.lock().expect("calls lock poisoned").clone()
        }
    }

    impl AssetRepository for RecordingAssetRepository {
        type Error = TestRepositoryError;

        fn insert<'a>(
            &'a self,
            asset: &'a Asset,
        ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
            self.calls
                .lock()
                .expect("calls lock poisoned")
                .push(RepositoryCall::Insert(asset.clone()));
            ready(self.insert_result)
        }

        fn find_by_id<'a>(
            &'a self,
            tenant_id: &'a TenantId,
            asset_id: &'a AssetId,
        ) -> impl Future<Output = Result<Option<Asset>, Self::Error>> + Send + 'a {
            self.calls
                .lock()
                .expect("calls lock poisoned")
                .push(RepositoryCall::FindById(
                    tenant_id.clone(),
                    asset_id.clone(),
                ));
            ready(self.find_result.clone())
        }
    }

    struct AllowAllPolicy;

    impl TenantAccessPolicy for AllowAllPolicy {
        type Error = Infallible;

        fn evaluate<'a>(
            &'a self,
            _request: &'a TenantAccessRequest,
        ) -> impl Future<Output = Result<AccessDecision, Self::Error>> + Send + 'a {
            ready(Ok(AccessDecision::Allow))
        }
    }

    #[tokio::test]
    async fn registers_asset_owned_by_the_authorized_tenant() {
        let repository = RecordingAssetRepository::default();
        let service = AssetApplicationService::new(repository.clone());
        let asset = asset("tenant-a", "asset-a");

        service
            .register(
                authorization("tenant-a", TenantAction::ManageMetadata).await,
                &asset,
            )
            .await
            .expect("matching tenant should be registered");

        assert_eq!(repository.calls(), vec![RepositoryCall::Insert(asset)]);
    }

    #[tokio::test]
    async fn rejects_wrong_action_and_tenant_before_persistence() {
        let repository = RecordingAssetRepository::default();
        let service = AssetApplicationService::new(repository.clone());
        let asset = asset("tenant-b", "asset-a");

        let wrong_action = service
            .register(
                authorization("tenant-b", TenantAction::ReadMetadata).await,
                &asset,
            )
            .await
            .expect_err("read authorization must not permit registration");
        assert!(matches!(
            wrong_action,
            AssetApplicationError::ActionRequired {
                required: TenantAction::ManageMetadata,
                actual: TenantAction::ReadMetadata,
            }
        ));

        let wrong_tenant = service
            .register(
                authorization("tenant-a", TenantAction::ManageMetadata).await,
                &asset,
            )
            .await
            .expect_err("cross-tenant asset must be rejected");
        assert!(matches!(
            wrong_tenant,
            AssetApplicationError::TenantMismatch {
                authorized_tenant_id,
                asset_tenant_id,
            } if authorized_tenant_id.as_str() == "tenant-a"
                && asset_tenant_id.as_str() == "tenant-b"
        ));
        assert!(repository.calls().is_empty());
    }

    #[tokio::test]
    async fn lookup_derives_tenant_scope_from_read_authorization() {
        let asset = asset("tenant-a", "asset-a");
        let repository = RecordingAssetRepository::returning(asset.clone());
        let service = AssetApplicationService::new(repository.clone());
        let asset_id = asset.id().clone();

        let found = service
            .find_by_id(
                authorization("tenant-a", TenantAction::ReadMetadata).await,
                &asset_id,
            )
            .await
            .expect("lookup should succeed");

        assert_eq!(found, Some(asset));
        assert_eq!(
            repository.calls(),
            vec![RepositoryCall::FindById(tenant_id("tenant-a"), asset_id)]
        );
    }

    #[tokio::test]
    async fn lookup_rejects_manage_authorization_before_persistence() {
        let repository = RecordingAssetRepository::default();
        let service = AssetApplicationService::new(repository.clone());

        let error = service
            .find_by_id(
                authorization("tenant-a", TenantAction::ManageMetadata).await,
                &AssetId::new("asset-a").expect("valid asset ID"),
            )
            .await
            .expect_err("manage authorization must not imply read authorization");

        assert!(matches!(
            error,
            AssetApplicationError::ActionRequired {
                required: TenantAction::ReadMetadata,
                actual: TenantAction::ManageMetadata,
            }
        ));
        assert!(repository.calls().is_empty());
    }

    #[tokio::test]
    async fn preserves_repository_failure_as_the_error_source() {
        let service = AssetApplicationService::new(RecordingAssetRepository::failing());
        let asset = asset("tenant-a", "asset-a");

        let error = service
            .register(
                authorization("tenant-a", TenantAction::ManageMetadata).await,
                &asset,
            )
            .await
            .expect_err("repository failure should be returned");

        assert!(matches!(
            error,
            AssetApplicationError::Repository(TestRepositoryError)
        ));
        assert!(error.source().is_some());
    }

    async fn authorization(tenant: &str, action: TenantAction) -> AuthorizedTenantAction {
        TenantAuthorizationService::new(AllowAllPolicy)
            .authorize(
                crate::test_support::authenticated_actor(
                    ActorId::new("actor-a").expect("valid actor ID"),
                )
                .await,
                tenant_id(tenant),
                action,
            )
            .await
            .expect("test policy should authorize request")
    }

    fn asset(tenant: &str, asset_id: &str) -> Asset {
        Asset::new(
            tenant_id(tenant),
            AssetId::new(asset_id).expect("valid asset ID"),
            AssetTypeRef::new(
                AssetTypeId::new("industrial.machine").expect("valid asset type ID"),
                DefinitionVersion::new(1).expect("valid definition version"),
            ),
            AssetName::new("Primary machine").expect("valid asset name"),
        )
    }

    fn tenant_id(value: &str) -> TenantId {
        TenantId::new(value).expect("valid tenant ID")
    }
}

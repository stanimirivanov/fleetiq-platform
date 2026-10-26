use std::{error::Error, fmt};

use fleetiq_domain::{Asset, AssetId, TenantId};

use crate::{
    AssetPage, AssetPageQuery, AssetRepository, AuthorizedTenant, ManageMetadata, ReadMetadata,
};

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
    /// Returns [`AssetApplicationError::TenantMismatch`] when the asset is
    /// outside the authorized tenant, or wraps the repository error when
    /// persistence fails.
    pub async fn register(
        &self,
        authorization: AuthorizedTenant<ManageMetadata>,
        asset: &Asset,
    ) -> Result<(), AssetApplicationError<R::Error>> {
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
    /// Wraps the repository error when the lookup fails.
    pub async fn find_by_id(
        &self,
        authorization: AuthorizedTenant<ReadMetadata>,
        asset_id: &AssetId,
    ) -> Result<Option<Asset>, AssetApplicationError<R::Error>> {
        self.repository
            .find_by_id(authorization.tenant_id(), asset_id)
            .await
            .map_err(AssetApplicationError::Repository)
    }

    /// Lists one bounded page using only the tenant in `ReadMetadata` evidence.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when the catalogue read fails.
    pub async fn list_page(
        &self,
        authorization: AuthorizedTenant<ReadMetadata>,
        query: &AssetPageQuery,
    ) -> Result<AssetPage, AssetApplicationError<R::Error>> {
        self.repository
            .list_page(authorization.tenant_id(), query)
            .await
            .map_err(AssetApplicationError::Repository)
    }
}

/// Failure to execute an authorized asset operation.
#[derive(Debug)]
pub enum AssetApplicationError<E> {
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
            Self::TenantMismatch { .. } => None,
        }
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
        ListPage(TenantId, AssetPageQuery),
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
        page_result: Result<AssetPage, TestRepositoryError>,
    }

    impl Default for RecordingAssetRepository {
        fn default() -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                insert_result: Ok(()),
                find_result: Ok(None),
                page_result: Ok(AssetPage::new(Vec::new(), None)),
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
                page_result: Err(TestRepositoryError),
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

        fn list_page<'a>(
            &'a self,
            tenant_id: &'a TenantId,
            query: &'a AssetPageQuery,
        ) -> impl Future<Output = Result<AssetPage, Self::Error>> + Send + 'a {
            self.calls
                .lock()
                .expect("calls lock poisoned")
                .push(RepositoryCall::ListPage(tenant_id.clone(), query.clone()));
            ready(self.page_result.clone())
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
            .register(authorization::<ManageMetadata>("tenant-a").await, &asset)
            .await
            .expect("matching tenant should be registered");

        assert_eq!(repository.calls(), vec![RepositoryCall::Insert(asset)]);
    }

    #[tokio::test]
    async fn rejects_wrong_tenant_before_persistence() {
        let repository = RecordingAssetRepository::default();
        let service = AssetApplicationService::new(repository.clone());
        let asset = asset("tenant-b", "asset-a");

        let wrong_tenant = service
            .register(authorization::<ManageMetadata>("tenant-a").await, &asset)
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
        assert_eq!(repository.calls().len(), 0);
    }

    #[tokio::test]
    async fn lookup_derives_tenant_scope_from_read_authorization() {
        let asset = asset("tenant-a", "asset-a");
        let repository = RecordingAssetRepository::returning(asset.clone());
        let service = AssetApplicationService::new(repository.clone());
        let asset_id = asset.id().clone();

        let found = service
            .find_by_id(authorization::<ReadMetadata>("tenant-a").await, &asset_id)
            .await
            .expect("lookup should succeed");

        assert_eq!(found, Some(asset));
        assert_eq!(
            repository.calls(),
            vec![RepositoryCall::FindById(tenant_id("tenant-a"), asset_id)]
        );
    }

    #[tokio::test]
    async fn page_derives_tenant_scope_from_read_authorization() {
        let repository = RecordingAssetRepository::default();
        let service = AssetApplicationService::new(repository.clone());
        let query = AssetPageQuery::new(25, Some(AssetId::new("asset-a").unwrap())).unwrap();

        let page = service
            .list_page(authorization::<ReadMetadata>("tenant-a").await, &query)
            .await
            .expect("catalogue read should succeed");

        assert_eq!(page.assets().len(), 0);
        assert_eq!(
            repository.calls(),
            vec![RepositoryCall::ListPage(tenant_id("tenant-a"), query)]
        );
    }

    #[tokio::test]
    async fn preserves_repository_failure_as_the_error_source() {
        let service = AssetApplicationService::new(RecordingAssetRepository::failing());
        let asset = asset("tenant-a", "asset-a");

        let error = service
            .register(authorization::<ManageMetadata>("tenant-a").await, &asset)
            .await
            .expect_err("repository failure should be returned");

        assert!(matches!(
            error,
            AssetApplicationError::Repository(TestRepositoryError)
        ));
        assert!(error.source().is_some());
    }

    async fn authorization<A: crate::TenantActionMarker>(tenant: &str) -> AuthorizedTenant<A> {
        TenantAuthorizationService::new(AllowAllPolicy)
            .authorize::<A>(
                crate::test_support::authenticated_actor(
                    ActorId::new("actor-a").expect("valid actor ID"),
                )
                .await,
                tenant_id(tenant),
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

use super::*;
use std::{future::ready, io};

#[test]
fn query_bounds_include_both_endpoints() {
    for limit in [0, 101, u16::MAX] {
        assert!(TenantDiscoveryQuery::new(limit, None).is_err());
    }
    for limit in [1, 100] {
        assert_eq!(
            TenantDiscoveryQuery::new(limit, None).unwrap().limit(),
            limit
        );
    }
}

struct Repository;
impl TenantDiscoveryRepository for Repository {
    type Error = io::Error;
    fn find_page<'a>(
        &'a self,
        actor: &'a ActorId,
        query: &'a TenantDiscoveryQuery,
    ) -> impl Future<Output = Result<TenantDiscoveryPage, Self::Error>> + Send + 'a {
        assert_eq!(actor.as_str(), "actor-a");
        assert_eq!(query.limit(), 1);
        assert_eq!(query.after().unwrap().as_str(), "tenant-a");
        ready(Err(io::Error::other("unavailable")))
    }
}

#[tokio::test]
async fn service_uses_authenticated_actor_and_preserves_repository_failure() {
    let actor = AuthenticatedActor::from_trusted_actor_id(ActorId::new("actor-a").unwrap());
    let query = TenantDiscoveryQuery::new(1, Some(TenantId::new("tenant-a").unwrap())).unwrap();
    let error = TenantDiscovery::new(Repository)
        .find_page(actor, &query)
        .await
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
}

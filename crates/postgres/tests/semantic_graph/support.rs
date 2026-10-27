use fleetiq_access::{
    AuthorizedTenant, CallerAuthenticationService, ExternalIdentityAuthenticator, ReadMetadata,
    TenantAuthorizationService,
};
use fleetiq_domain::{
    AssetId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId, TenantId,
    UnixTimeMillis,
};
use fleetiq_metadata::{GraphBudget, SemanticGraphQuery};
use fleetiq_postgres::{PostgresActorIdentityResolver, PostgresTenantAccessPolicy};
use sqlx::PgPool;
use std::{
    convert::Infallible,
    error::Error,
    future::{Future, ready},
};
struct Authenticator;
impl ExternalIdentityAuthenticator for Authenticator {
    type Credential = ();
    type Error = Infallible;
    fn authenticate(
        &self,
        (): &(),
    ) -> impl Future<Output = Result<Option<ExternalIdentity>, Infallible>> + Send {
        ready(Ok(Some(ExternalIdentity::new(
            IdentityAuthorityId::new("workload").unwrap(),
            ExternalIdentitySubject::new("operator").unwrap(),
        ))))
    }
}
pub(super) async fn setup(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(include_str!("topology.sql"))
        .execute(pool)
        .await?;
    Ok(())
}
pub(super) async fn grant(
    pool: &PgPool,
    tenant: &str,
) -> Result<AuthorizedTenant<ReadMetadata>, Box<dyn Error>> {
    let actor = CallerAuthenticationService::new(
        Authenticator,
        PostgresActorIdentityResolver::new(pool.clone()),
    )
    .authenticate(&())
    .await?;
    Ok(
        TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone()))
            .authorize(actor, TenantId::new(tenant)?)
            .await?,
    )
}
pub(super) fn query(effective: i64, known: i64, budget: GraphBudget) -> SemanticGraphQuery {
    SemanticGraphQuery::new(
        AssetId::new("asset-a").unwrap(),
        UnixTimeMillis::new(effective),
        UnixTimeMillis::new(known),
        budget,
    )
}

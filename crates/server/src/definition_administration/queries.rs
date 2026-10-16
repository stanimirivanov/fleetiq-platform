//! Small bounded summaries; complete aggregate reads keep their existing routes.
use super::{
    DefinitionState,
    dto::{CatalogueCursor, CatalogueEntry, CatalogueParameters, CatalogueResponse},
    internal, invalid,
};
use crate::protected_request::{RequestAuthenticator, no_store};
use axum::{
    Extension, Json,
    extract::{
        Path, Query, State,
        rejection::{PathRejection, QueryRejection},
    },
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use fleetiq_access::{AuthorizedTenant, ReadMetadata};
use fleetiq_metadata::DefinitionKind;
use std::sync::Arc;
pub(super) async fn catalogue<A: RequestAuthenticator>(
    State(state): State<Arc<DefinitionState<A>>>,
    Extension(kind): Extension<DefinitionKind>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    parameters: Result<Query<CatalogueParameters>, QueryRejection>,
) -> Response {
    let authorization = match state
        .authorize::<ReadMetadata>(
            &headers,
            path.as_ref().ok().map(|Path(id)| id.as_str()),
            false,
        )
        .await
    {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    respond(&state, authorization, kind, None, parameters).await
}
pub(super) async fn versions<A: RequestAuthenticator>(
    State(state): State<Arc<DefinitionState<A>>>,
    Extension(kind): Extension<DefinitionKind>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    parameters: Result<Query<CatalogueParameters>, QueryRejection>,
) -> Response {
    let authorization = match state
        .authorize::<ReadMetadata>(
            &headers,
            path.as_ref().ok().map(|Path((tenant, _))| tenant.as_str()),
            false,
        )
        .await
    {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let Ok(Path((_, identity))) = path else {
        return invalid();
    };
    respond(&state, authorization, kind, Some(identity), parameters).await
}
async fn respond<A>(
    state: &DefinitionState<A>,
    authorization: AuthorizedTenant<ReadMetadata>,
    kind: DefinitionKind,
    identity: Option<String>,
    parameters: Result<Query<CatalogueParameters>, QueryRejection>,
) -> Response {
    let Ok(Query(parameters)) = parameters else {
        return invalid();
    };
    let Some(query) = parameters.query(kind, identity) else {
        return invalid();
    };
    match state.catalogue.list_page(authorization, &query).await {
        Ok(page) => no_store(
            Json(CatalogueResponse {
                definitions: page
                    .entries()
                    .iter()
                    .map(|entry| CatalogueEntry {
                        id: entry.position().identity().as_str(),
                        version: entry.position().version().get(),
                        name: entry.name().as_str(),
                    })
                    .collect(),
                next_after: page.next().map(|p| CatalogueCursor {
                    id: p.identity().as_str(),
                    version: p.version().get(),
                }),
            })
            .into_response(),
        ),
        Err(_) => internal(),
    }
}

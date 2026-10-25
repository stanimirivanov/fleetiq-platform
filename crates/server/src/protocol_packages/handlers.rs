use super::{
    State,
    dto::{self, AdmissionBody, AdmissionResponse, ReleaseBody},
};
use crate::protected_request::{self, ProtectedRequestError, RequestAuthenticator};
use axum::{
    Json,
    body::Bytes,
    extract::{
        Path, State as ExtractState,
        rejection::{BytesRejection, JsonRejection, PathRejection},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use fleetiq_ingestion::{
    ProtocolArtifact, ProtocolCompiler, ProtocolCompilerError, ProtocolPackageRegistrationOutcome,
    ProtocolPackageServiceError,
};
use fleetiq_postgres::{PostgresProtocolPackageCatalogError, PostgresRepositoryError};
use std::sync::Arc;
type PackagePath = Result<Path<(String, String, String)>, PathRejection>;
type Failure =
    ProtocolPackageServiceError<PostgresProtocolPackageCatalogError, PostgresRepositoryError>;
fn problem(status: StatusCode, title: &'static str) -> Response {
    protected_request::problem(status, "urn:fleetiq:problem:protocol-package", title)
}
pub(super) fn failure(error: &Failure) -> Response {
    match error {
        Failure::NotFound => problem(StatusCode::NOT_FOUND, "Protocol package not found"),
        Failure::ContentMismatch => problem(StatusCode::BAD_REQUEST, "Artifact identity differs"),
        Failure::Catalog(PostgresProtocolPackageCatalogError::IdentityCollision { .. }) => {
            problem(StatusCode::CONFLICT, "Release identity is occupied")
        }
        Failure::Compiler(ProtocolCompilerError::Rejected) => problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Protocol component rejected",
        ),
        Failure::Compiler(ProtocolCompilerError::Busy) => problem(
            StatusCode::TOO_MANY_REQUESTS,
            "Admission capacity is occupied",
        ),
        Failure::Compiler(ProtocolCompilerError::Unavailable | ProtocolCompilerError::Timeout) => {
            problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "Admission worker unavailable",
            )
        }
        Failure::Catalog(_) | Failure::Store(_) => ProtectedRequestError::Internal.into_response(),
    }
}
pub(super) async fn find<A: RequestAuthenticator, V: ProtocolCompiler>(
    ExtractState(state): ExtractState<Arc<State<A, V>>>,
    headers: HeaderMap,
    path: PackagePath,
) -> Response {
    let auth = match state
        .authorize(
            &headers,
            path.as_ref()
                .ok()
                .map(|Path((tenant, _, _))| tenant.as_str()),
            false,
        )
        .await
    {
        Ok(auth) => auth,
        Err(e) => return e.into_response(),
    };
    let Ok(Path((_, id, version))) = path else {
        return problem(StatusCode::BAD_REQUEST, "Invalid package identity");
    };
    let Some(package) = dto::package(&id, &version) else {
        return problem(StatusCode::BAD_REQUEST, "Invalid package identity");
    };
    match state.packages.find(auth, &package).await {
        Ok(Some(release)) => {
            protected_request::no_store(Json(ReleaseBody::from(&release)).into_response())
        }
        Ok(None) => problem(StatusCode::NOT_FOUND, "Protocol package not found"),
        Err(e) => failure(&e),
    }
}
pub(super) async fn register<A: RequestAuthenticator, V: ProtocolCompiler>(
    ExtractState(state): ExtractState<Arc<State<A, V>>>,
    headers: HeaderMap,
    path: PackagePath,
    bytes: Result<Bytes, BytesRejection>,
) -> Response {
    let auth = match state
        .authorize(
            &headers,
            path.as_ref()
                .ok()
                .map(|Path((tenant, _, _))| tenant.as_str()),
            true,
        )
        .await
    {
        Ok(auth) => auth,
        Err(e) => return e.into_response(),
    };
    if headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        != Some("application/wasm")
    {
        return problem(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Expected application/wasm",
        );
    }
    let Ok(Path((_, id, version))) = path else {
        return problem(StatusCode::BAD_REQUEST, "Invalid package identity");
    };
    let Some(package) = dto::package(&id, &version) else {
        return problem(StatusCode::BAD_REQUEST, "Invalid package identity");
    };
    let Some(digest) = headers
        .get("x-artifact-sha256")
        .and_then(|v| v.to_str().ok())
        .and_then(dto::digest)
    else {
        return problem(StatusCode::BAD_REQUEST, "Invalid artifact digest");
    };
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(e) => return problem(e.status(), "Artifact body rejected"),
    };
    let Ok(artifact) = ProtocolArtifact::new(bytes.to_vec()) else {
        return problem(StatusCode::BAD_REQUEST, "Invalid artifact size");
    };
    match state
        .packages
        .register(auth, package, digest, &artifact)
        .await
    {
        Ok((release, outcome)) => protected_request::no_store(
            (
                if outcome == ProtocolPackageRegistrationOutcome::Registered {
                    StatusCode::CREATED
                } else {
                    StatusCode::OK
                },
                Json(ReleaseBody::from(&release)),
            )
                .into_response(),
        ),
        Err(e) => failure(&e),
    }
}
pub(super) async fn admit<A: RequestAuthenticator, V: ProtocolCompiler>(
    ExtractState(state): ExtractState<Arc<State<A, V>>>,
    headers: HeaderMap,
    path: PackagePath,
    body: Result<Json<AdmissionBody>, JsonRejection>,
) -> Response {
    let auth = match state
        .authorize(
            &headers,
            path.as_ref()
                .ok()
                .map(|Path((tenant, _, _))| tenant.as_str()),
            true,
        )
        .await
    {
        Ok(auth) => auth,
        Err(e) => return e.into_response(),
    };
    let (Ok(Path((_, id, version))), Ok(Json(body))) = (path, body) else {
        return problem(StatusCode::BAD_REQUEST, "Invalid admission request");
    };
    let Some(package) = dto::package(&id, &version) else {
        return problem(StatusCode::BAD_REQUEST, "Invalid package identity");
    };
    let now = match protected_request::now() {
        Ok(now) => now,
        Err(e) => return e.into_response(),
    };
    match state
        .packages
        .admit(auth, &package, body.world.into(), now)
        .await
    {
        Ok(receipt) => {
            protected_request::no_store(Json(AdmissionResponse::from(&receipt)).into_response())
        }
        Err(e) => failure(&e),
    }
}

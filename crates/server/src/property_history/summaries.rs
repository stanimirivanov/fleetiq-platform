//! Scope-bound revision-pinned paging of derived evidence; raw history is unchanged.
use super::{
    Context, budget,
    handlers::{authorize, invalid, millis, target},
};
use crate::{
    cursor_codec,
    protected_request::{self, ProtectedRequestError, RequestAuthenticator},
};
use axum::{
    Json,
    extract::{
        Path, Query, State,
        rejection::{PathRejection, QueryRejection},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use fleetiq_history::{
    downsampling::{NumericRequest, NumericSpec, SummaryReceipt},
    restore::RestoreTicket,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Parameters {
    start_at_ms: String,
    end_at_ms: String,
    resolution_ms: String,
    limit: Option<u8>,
    after: Option<String>,
    manifest_id: Option<String>,
    restore_generation: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Position {
    scope: Vec<u8>,
    series: i64,
    revision: i64,
    offset: usize,
}
pub(super) async fn list<A: RequestAuthenticator>(
    State(ctx): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String, String, String)>, PathRejection>,
    parameters: Result<Query<Parameters>, QueryRejection>,
) -> Result<Response, ProtectedRequestError> {
    let grant = authorize(&ctx, &headers, path.as_ref().map_or("", |p| p.0.0.as_str())).await?;
    let (Ok(Path((_, asset, property, version))), Ok(Query(p))) = (path, parameters) else {
        return Ok(invalid());
    };
    let Some(target) = target(asset, property, &version) else {
        return Ok(invalid());
    };
    let (Some(start), Some(end), Some(width)) = (
        millis(&p.start_at_ms),
        millis(&p.end_at_ms),
        millis(&p.resolution_ms),
    ) else {
        return Ok(invalid());
    };
    let Ok(spec) = NumericSpec::new(start, end, width) else {
        return Ok(invalid());
    };
    let limit = usize::from(p.limit.unwrap_or(64));
    if !(1..=64).contains(&limit) {
        return Ok(invalid());
    }
    let Ok(selection) = p.selection() else {
        return Ok(invalid());
    };
    let tenant = grant.tenant_id().clone();
    let bytes = serde_json::to_vec(&(
        tenant.as_str(),
        target.asset_id().as_str(),
        target.property().id().as_str(),
        target.property().version().get(),
        start,
        end,
        width,
        selection.map(RestoreTicket::manifest),
        selection.map(RestoreTicket::generation),
    ))
    .map_err(|_| ProtectedRequestError::Internal)?;
    let scope = Sha256::digest(bytes).to_vec();
    let position = if let Some(after) = p.after {
        let Some(pos) = cursor_codec::decode::<Position>(&after) else {
            return Ok(invalid());
        };
        if pos.scope != scope
            || pos.series <= 0
            || pos.revision <= 0
            || pos.offset == 0
            || pos.offset
                >= usize::try_from((end - start) / width)
                    .map_err(|_| ProtectedRequestError::Internal)?
        {
            return Ok(invalid());
        }
        Some(pos)
    } else {
        None
    };
    let request = NumericRequest::new(tenant.clone(), target, spec, selection);
    budget::run(&ctx.budgets, &tenant, async {
        let pinned = position.as_ref().map(|p| SummaryReceipt { series: p.series, revision: p.revision });
        let summary = ctx.summaries.read(grant, &request, pinned).await
            .map_err(|_| ProtectedRequestError::Internal)?;
        let Some(summary) = summary else {
            return Ok(if position.is_some() { invalid() } else {
                protected_request::problem(StatusCode::NOT_FOUND, protected_request::NOT_FOUND, "Resource not found")
            });
        };
        let offset = position.as_ref().map_or(0, |p| p.offset);
        if offset >= summary.buckets.len() { return Ok(invalid()); }
        let end = (offset + limit).min(summary.buckets.len());
        let next_after = if end < summary.buckets.len() {
            Some(cursor_codec::encode(&Position { scope, series: summary.receipt.series, revision: summary.receipt.revision, offset: end })
                .map_err(|_| ProtectedRequestError::Internal)?)
        } else { None };
        let buckets = summary.buckets[offset..end].iter().map(|b| json!({
            "start_ms": b.start.to_string(), "end_ms": b.end.to_string(),
            "total": b.total, "count": b.count, "excluded": b.excluded,
            "min": b.minimum.as_ref().map(|v| v.normalized().to_plain_string()),
            "max": b.maximum.as_ref().map(|v| v.normalized().to_plain_string()),
            "sum": b.sum.normalized().to_plain_string()
        })).collect::<Vec<_>>();
        let body = json!({
            "representation": "numeric_summary", "algorithm": "numeric-fixed-v1",
            "series_id": summary.receipt.series.to_string(), "revision": summary.receipt.revision.to_string(),
            "observed_at_ms": summary.observed_at_ms.to_string(),
            "start_at_ms": start.to_string(), "end_at_ms": spec.end().to_string(), "resolution_ms": width.to_string(),
            "kind": if summary.kind == fleetiq_domain::PropertyValueKind::Integer { "integer" } else { "decimal" },
            "unit": summary.unit.as_ref().map(fleetiq_domain::UnitId::as_str),
            "coverage": {
                "scope": if selection.is_some() { "hot_and_selected_manifest_snapshot" } else { "hot_snapshot" },
                "manifest_id": selection.map(|t| t.manifest().to_string()),
                "restore_generation": selection.map(|t| t.generation().to_string())
            },
            "buckets": buckets, "next_after": next_after
        });
        Ok(protected_request::no_store(Json(body).into_response()))
    }).await
}

impl Parameters {
    fn selection(&self) -> Result<Option<RestoreTicket>, ()> {
        match (
            self.manifest_id.as_deref(),
            self.restore_generation.as_deref(),
        ) {
            (None, None) => Ok(None),
            (Some(manifest), Some(generation)) => Ok(Some(
                RestoreTicket::new(millis(manifest).ok_or(())?, millis(generation).ok_or(())?)
                    .map_err(|_| ())?,
            )),
            _ => Err(()),
        }
    }
}

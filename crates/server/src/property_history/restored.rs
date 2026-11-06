//! Explicit selected-restore coverage; fresh authority is established by the owning handler.
use super::{Context, budget, cursor, dto, handlers};
use crate::protected_request::{self, ProtectedRequestError, RequestAuthenticator};
use axum::{
    http::header,
    response::{IntoResponse, Response},
};
use fleetiq_access::{AuthorizedTenant, ReadTelemetry};
use fleetiq_history::{PropertyHistoryQuery, RestoredHistorySnapshot, restore::RestoreTicket};
pub(super) fn selection(parameters: &dto::Parameters) -> Result<Option<RestoreTicket>, ()> {
    match (
        parameters.mode.as_deref(),
        parameters.manifest_id.as_deref(),
        parameters.restore_generation.as_deref(),
    ) {
        (None | Some("hot"), None, None) => Ok(None),
        (Some("hot_and_restored"), Some(manifest), Some(generation)) => {
            let canonical = |s: &str| s.parse::<i64>().ok().filter(|n| n.to_string() == s);
            RestoreTicket::new(
                canonical(manifest).ok_or(())?,
                canonical(generation).ok_or(())?,
            )
            .map(Some)
            .map_err(|_| ())
        }
        _ => Err(()),
    }
}
pub(super) async fn list<A: RequestAuthenticator>(
    ctx: &Context<A>,
    grant: AuthorizedTenant<ReadTelemetry>,
    query: &PropertyHistoryQuery,
    ticket: RestoreTicket,
    epoch: Option<&str>,
) -> Result<Response, ProtectedRequestError> {
    let tenant = grant.tenant_id().clone();
    let snapshot = ctx
        .restores
        .history_snapshot(&tenant, ticket)
        .await
        .map_err(|_| ProtectedRequestError::Internal)?;
    let (coverage, restored) = match snapshot {
        RestoredHistorySnapshot::Archived => ("archived", Vec::new()),
        RestoredHistorySnapshot::Restoring => ("restoring", Vec::new()),
        RestoredHistorySnapshot::Unavailable => ("unavailable", Vec::new()),
        RestoredHistorySnapshot::Ready(bytes) => match fleetiq_archive::decode(&tenant, &bytes) {
            Ok(rows) if rows.len() <= 16 => ("restored", rows),
            _ => ("unavailable", Vec::new()),
        },
    };
    if epoch.is_some_and(|e| e != coverage || coverage == "unavailable") {
        return Ok(handlers::invalid());
    }
    let page = ctx
        .history
        .list_with_restored(grant, query, &restored)
        .await
        .map_err(|_| ProtectedRequestError::Internal)?;
    let next_after = if coverage == "unavailable" {
        None
    } else {
        page.next_after()
            .map(|key| {
                cursor::restored_page_token(&tenant, query, key, ticket, coverage)
                    .ok_or(ProtectedRequestError::Internal)
            })
            .transpose()?
    };
    let changes = page
        .changes()
        .iter()
        .map(dto::ChangeBody::new)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ProtectedRequestError::Internal)?;
    let bytes = serde_json::to_vec(&dto::HistoryPageBody {
        changes,
        next_after,
        coverage: Some(dto::HistoryCoverageBody {
            hot: "queried",
            archive: coverage,
            scope: "selected_manifest_only",
            manifest_id: ticket.manifest().to_string(),
            restore_generation: ticket.generation().to_string(),
        }),
    })
    .map_err(|_| ProtectedRequestError::Internal)?;
    if bytes.len() > budget::RESPONSE_BYTES {
        return Ok(budget::denied());
    }
    Ok(protected_request::no_store(
        ([(header::CONTENT_TYPE, "application/json")], bytes).into_response(),
    ))
}

pub(super) fn page_position(
    token: Option<&str>,
    tenant: &fleetiq_domain::TenantId,
    target: &fleetiq_domain::SignalTarget,
    start: i64,
    end: i64,
    ticket: Option<RestoreTicket>,
) -> Result<
    (
        Option<fleetiq_history::PropertyHistoryCursor>,
        Option<String>,
    ),
    (),
> {
    let Some(token) = token else {
        return Ok((None, None));
    };
    if let Some(ticket) = ticket {
        cursor::restored_page_key(token, tenant, target, start, end, ticket)
            .map(|(key, state)| (Some(key), Some(state)))
            .ok_or(())
    } else {
        cursor::page_key(token, tenant, target, start, end)
            .map(|key| (Some(key), None))
            .ok_or(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn modes_require_complete_canonical_selection() {
        for (mode, manifest, generation) in [
            ("unknown", Some("1"), Some("1")),
            ("hot", Some("1"), Some("1")),
            ("hot_and_restored", Some("1"), None),
            ("hot_and_restored", Some("01"), Some("1")),
            ("hot_and_restored", Some("1"), Some("0")),
            ("hot_and_restored", Some("9223372036854775808"), Some("1")),
        ] {
            let p:dto::Parameters=serde_json::from_value(json!({"start_at_ms":"0","end_at_ms":"1","mode":mode,"manifest_id":manifest,"restore_generation":generation})).unwrap();
            assert!(selection(&p).is_err());
        }
    }
}

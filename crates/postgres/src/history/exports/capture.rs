//! Repeatable-read source capture; verified coverage and exact overlap precede freezing.
use super::{ExportCodec, ExportFailure, ExportRequest, PostgresExports};
use crate::history::{HistoryRow, property_change_log::interpretation_key as key};
use serde_json::Value;
impl PostgresExports {
    pub(super) async fn capture<C: ExportCodec>(
        &self,
        req: &ExportRequest,
        reservation: i64,
        codec: &C,
    ) -> Result<(i32, Vec<u8>), ExportFailure> {
        let mut tx = self.0.begin().await.map_err(|_| ExportFailure::Storage)?;
        sqlx::raw_sql("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY; SET LOCAL statement_timeout='2s';").execute(&mut *tx).await.map_err(|_|ExportFailure::Storage)?;
        let q = req.query();
        let target = q.target();
        let ticket = req.restore();
        let meta = sqlx::query_file!(
            "src/history/exports/context.sql",
            req.tenant().as_str(),
            target.asset_id().as_str(),
            target.property().id().as_str(),
            i64::from(target.property().version().get()),
            ticket.map(fleetiq_history::restore::RestoreTicket::manifest),
            reservation
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ExportFailure::Storage)?
        .ok_or(ExportFailure::Disabled)?;
        if !meta.active {
            return Err(ExportFailure::Disabled);
        }
        if !meta.admitted {
            return Err(ExportFailure::Denied);
        }
        if !meta.target_present {
            return Err(ExportFailure::Bounds);
        }
        if meta.missing {
            return Err(ExportFailure::Coverage);
        }
        let hot = sqlx::query_file_as!(
            HistoryRow,
            "src/history/exports/source.sql",
            req.tenant().as_str(),
            target.asset_id().as_str(),
            target.property().id().as_str(),
            i64::from(target.property().version().get()),
            q.start().get(),
            q.end().get(),
            q.start().get(),
            "",
            "",
            "",
            -1_i64,
            "",
            bigdecimal::BigDecimal::from(0),
            "",
            17_i64
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| ExportFailure::Storage)?;
        if hot.len() > 16
            || hot.iter().any(|r| {
                (r.value_kind == "decimal" && r.value_decimal.is_none())
                    || (r.value_kind == "text" && r.value_text.is_none())
            })
        {
            return Err(ExportFailure::Limit);
        }
        let mut rows = hot
            .into_iter()
            .map(|r| {
                r.into_domain(req.tenant())
                    .map_err(|_| ExportFailure::Coverage)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(ticket) = ticket {
            let restored = restored_rows(&mut tx, req, ticket, codec).await?;
            for r in restored.into_iter().filter(|r| {
                r.target() == target && r.event_at() >= q.start() && r.event_at() < q.end()
            }) {
                if let Some(old) = rows.iter().find(|old| old.id() == r.id()) {
                    if old != &r {
                        return Err(ExportFailure::Coverage);
                    }
                } else {
                    rows.push(r);
                }
            }
        }
        tx.commit().await.map_err(|_| ExportFailure::Storage)?;
        if rows.is_empty() || rows.len() > 16 {
            return Err(ExportFailure::Limit);
        }
        rows.sort_by_cached_key(|r| (r.event_at().get(), key(r).to_string()));
        let payload = codec.encode(req.tenant(), &rows)?;
        if payload.is_empty() || payload.len() > 2_097_152 {
            return Err(ExportFailure::Limit);
        }
        Ok((
            i32::try_from(rows.len()).map_err(|_| ExportFailure::Limit)?,
            payload,
        ))
    }
}

async fn restored_rows<C: ExportCodec>(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    req: &ExportRequest,
    ticket: fleetiq_history::restore::RestoreTicket,
    codec: &C,
) -> Result<Vec<fleetiq_history::StoredPropertyChange>, ExportFailure> {
    let state = sqlx::query_file!(
        "src/history/restore/history_snapshot.sql",
        req.tenant().as_str(),
        ticket.manifest(),
        ticket.generation()
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ExportFailure::Storage)?
    .ok_or(ExportFailure::Coverage)?;
    if state.state != "restored" {
        return Err(ExportFailure::Coverage);
    }
    let file = state.payload.ok_or(ExportFailure::Coverage)?;
    let restored = codec.decode(req.tenant(), &file)?;
    let members:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_array(message_id,protocol_package_id,protocol_package_version,output_index,binding_id,trunc(binding_revision)::text,event_time_basis) FROM history.archive_members WHERE tenant_id=$1 AND job_id=$2").bind(req.tenant().as_str()).bind(ticket.manifest()).fetch_all(&mut **tx).await.map_err(|_|ExportFailure::Storage)?;
    if restored.len() > 16
        || restored.len() != members.len()
        || restored.iter().any(|r| r.tenant_id() != req.tenant())
        || members
            .iter()
            .any(|id| !restored.iter().any(|r| key(r) == *id))
    {
        return Err(ExportFailure::Coverage);
    }
    Ok(restored)
}

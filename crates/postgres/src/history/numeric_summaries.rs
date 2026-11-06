//! Bounded source snapshots and immutable serialized numeric publications.
use super::HistoryRow;
use fleetiq_domain::{PropertyValue, PropertyValueKind, UnitId};
use fleetiq_history::restore::RestoreTicket;
use fleetiq_history::{
    StoredPropertyChange,
    downsampling::{
        NumericRequest, NumericSample, SummaryArchiveDecoder, SummaryFailure, SummaryReceipt,
        summarize,
    },
};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};
/// Captured source/coverage proof. Fields are sealed; only the trusted decoder consumes its file.
pub struct NumericSnapshot {
    request: NumericRequest,
    rows: Vec<StoredPropertyChange>,
    kind: PropertyValueKind,
    unit: Option<UnitId>,
    observed: i64,
    reservation: i64,
    file: Option<Vec<u8>>,
    members: Vec<Value>,
}
/// Trusted producer persistence; construction starts no work.
pub struct PostgresNumericSummaries(PgPool);
fn key(c: &StoredPropertyChange) -> Value {
    let id = c.id();
    let o = id.observation_id();
    let p = o.decoded_by();
    json!([
        o.message_id().as_str(),
        p.id().as_str(),
        p.version().as_str(),
        o.output_index(),
        id.binding_id().as_str(),
        id.binding_revision().get().to_string(),
        super::property_change_log::basis_name(id.event_time_basis())
    ])
}
impl PostgresNumericSummaries {
    /// Uses an already migrated pool, without scheduling or activating policy.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    /// Captures at most 101 hot keys and one ready selected restore in a repeatable-read snapshot.
    /// # Errors
    /// Returns explicit admission, coverage, numeric-context and storage failures.
    pub async fn capture(
        &self,
        request: &NumericRequest,
        reservation: i64,
    ) -> Result<NumericSnapshot, SummaryFailure> {
        let mut tx = self.0.begin().await.map_err(|_| SummaryFailure::Storage)?;
        sqlx::raw_sql("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY; SET LOCAL statement_timeout='2s';").execute(&mut *tx).await.map_err(|_|SummaryFailure::Storage)?;
        let target = request.target();
        let spec = request.spec();
        let ticket = request.restore();
        let meta = sqlx::query_file!(
            "src/history/numeric_summaries/context.sql",
            request.tenant().as_str(),
            target.asset_id().as_str(),
            target.property().id().as_str(),
            i64::from(target.property().version().get()),
            ticket.map(RestoreTicket::manifest),
            reservation
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| SummaryFailure::Storage)?
        .ok_or(SummaryFailure::Disabled)?;
        if !meta.active {
            return Err(SummaryFailure::Disabled);
        }
        if !meta.target_present {
            return Err(SummaryFailure::InvalidEvidence);
        }
        if !meta.admitted {
            return Err(SummaryFailure::BudgetDenied);
        }
        if meta.missing {
            return Err(SummaryFailure::CoverageUnavailable);
        }
        let kind = match meta
            .value_kind
            .as_deref()
            .ok_or(SummaryFailure::InvalidEvidence)?
        {
            "integer" => PropertyValueKind::Integer,
            "decimal" => PropertyValueKind::Decimal,
            _ => return Err(SummaryFailure::InvalidEvidence),
        };
        let unit = meta
            .canonical_unit
            .map(UnitId::new)
            .transpose()
            .map_err(|_| SummaryFailure::InvalidEvidence)?;
        let rows = sqlx::query_file_as!(
            HistoryRow,
            "src/history/numeric_summaries/source.sql",
            request.tenant().as_str(),
            target.asset_id().as_str(),
            target.property().id().as_str(),
            i64::from(target.property().version().get()),
            spec.start(),
            spec.end(),
            spec.start(),
            "",
            "",
            "",
            -1_i64,
            "",
            bigdecimal::BigDecimal::from(0),
            "",
            101_i64
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| SummaryFailure::Storage)?;
        if rows.len() > 100 {
            return Err(SummaryFailure::InputLimit);
        }
        if rows
            .iter()
            .any(|r| r.value_kind == "decimal" && r.value_decimal.is_none())
        {
            return Err(SummaryFailure::InputLimit);
        }
        let rows = rows
            .into_iter()
            .map(|r| r.into_domain(request.tenant()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| SummaryFailure::InvalidEvidence)?;
        let (file, members) = capture_restore(&mut tx, request).await?;
        tx.commit().await.map_err(|_| SummaryFailure::Storage)?;
        Ok(NumericSnapshot {
            request: request.clone(),
            rows,
            kind,
            unit,
            observed: meta.observed,
            reservation,
            file,
            members,
        })
    }
    /// Reduces the captured snapshot with the trusted decoder's complete selected-file evidence.
    /// New arrivals are deliberately captured by a later run, never by rereading this snapshot.
    /// # Errors
    /// Rejects conflicting/missing lineage, quota changes, persistence faults or oversized output.
    pub async fn publish<D: SummaryArchiveDecoder>(
        &self,
        mut snapshot: NumericSnapshot,
        decoder: &D,
    ) -> Result<SummaryReceipt, SummaryFailure> {
        let body = summary_body(&mut snapshot, decoder)?;
        let req = &snapshot.request;
        let ticket = req.restore();
        let mut tx = self.0.begin().await.map_err(|_| SummaryFailure::Storage)?;
        sqlx::raw_sql("SET TRANSACTION ISOLATION LEVEL READ COMMITTED; SET LOCAL statement_timeout='2s'; SET LOCAL lock_timeout='2s';").execute(&mut *tx).await.map_err(|_|SummaryFailure::Storage)?;
        let active: bool = sqlx::query_scalar(
            "SELECT is_active FROM metadata.tenants WHERE tenant_id=$1 FOR UPDATE",
        )
        .bind(req.tenant().as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| SummaryFailure::Storage)?;
        if !active {
            return Err(SummaryFailure::Disabled);
        }
        let live:Option<i64>=sqlx::query_scalar("SELECT reservation_id FROM history.work_reservations WHERE tenant_id=$1 AND reservation_id=$2 AND reserved_bytes>=2097152 AND expires_at>clock_timestamp() AND policy_revision=(SELECT max(revision) FROM history.lifecycle_policies WHERE tenant_id=$1) FOR UPDATE").bind(req.tenant().as_str()).bind(snapshot.reservation).fetch_optional(&mut *tx).await.map_err(|_|SummaryFailure::Storage)?;
        if live.is_none() {
            return Err(SummaryFailure::BudgetDenied);
        }
        let target = req.target();
        let spec = req.spec();
        sqlx::query("INSERT INTO history.numeric_summary_series(tenant_id,asset_id,property_id,property_version,start_ms,end_ms,width_ms,manifest_id,restore_generation) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT ON CONSTRAINT uq_numeric_summary_scope DO NOTHING")
   .bind(req.tenant().as_str()).bind(target.asset_id().as_str()).bind(target.property().id().as_str()).bind(i64::from(target.property().version().get())).bind(spec.start()).bind(spec.end()).bind(spec.width()).bind(ticket.map(RestoreTicket::manifest)).bind(ticket.map(RestoreTicket::generation)).execute(&mut *tx).await.map_err(|_|SummaryFailure::Storage)?;
        let head=sqlx::query("SELECT series_id,revision FROM history.numeric_summary_series WHERE tenant_id=$1 AND asset_id=$2 AND property_id=$3 AND property_version=$4 AND start_ms=$5 AND end_ms=$6 AND width_ms=$7 AND algorithm=1 AND manifest_id IS NOT DISTINCT FROM $8 AND restore_generation IS NOT DISTINCT FROM $9 FOR UPDATE")
   .bind(req.tenant().as_str()).bind(target.asset_id().as_str()).bind(target.property().id().as_str()).bind(i64::from(target.property().version().get())).bind(spec.start()).bind(spec.end()).bind(spec.width()).bind(ticket.map(RestoreTicket::manifest)).bind(ticket.map(RestoreTicket::generation)).fetch_one(&mut *tx).await.map_err(|_|SummaryFailure::Storage)?;
        let series: i64 = head
            .try_get("series_id")
            .map_err(|_| SummaryFailure::Storage)?;
        let previous=sqlx::query("SELECT revision,body FROM history.numeric_summaries WHERE tenant_id=$1 AND series_id=$2 AND fingerprint=sha256(convert_to($3::jsonb::text,'UTF8'))").bind(req.tenant().as_str()).bind(series).bind(&body).fetch_optional(&mut *tx).await.map_err(|_|SummaryFailure::Storage)?;
        if let Some(row) = previous {
            if row
                .try_get::<Value, _>("body")
                .map_err(|_| SummaryFailure::Storage)?
                != body
            {
                return Err(SummaryFailure::InvalidEvidence);
            }
            let receipt = SummaryReceipt {
                series,
                revision: row
                    .try_get("revision")
                    .map_err(|_| SummaryFailure::Storage)?,
            };
            tx.commit().await.map_err(|_| SummaryFailure::Storage)?;
            return Ok(receipt);
        }
        let contains_previous:bool=sqlx::query_scalar("SELECT COALESCE((SELECT ($3::jsonb->'inputs') @> (body->'inputs') FROM history.numeric_summaries WHERE tenant_id=$1 AND series_id=$2 ORDER BY revision DESC LIMIT 1),true)")
            .bind(req.tenant().as_str()).bind(series).bind(&body).fetch_one(&mut *tx).await.map_err(|_|SummaryFailure::Storage)?;
        if !contains_previous {
            return Err(SummaryFailure::StaleSnapshot);
        }
        let revision = head
            .try_get::<i64, _>("revision")
            .map_err(|_| SummaryFailure::Storage)?
            .checked_add(1)
            .ok_or(SummaryFailure::Storage)?;
        sqlx::query("INSERT INTO history.numeric_summaries(tenant_id,series_id,revision,observed_at_ms,reservation_id,body,fingerprint) VALUES($1,$2,$3,$4,$5,$6,sha256(convert_to($6::jsonb::text,'UTF8')))")
   .bind(req.tenant().as_str()).bind(series).bind(revision).bind(snapshot.observed).bind(snapshot.reservation).bind(&body).execute(&mut *tx).await.map_err(|_|SummaryFailure::Storage)?;
        sqlx::query("UPDATE history.numeric_summary_series SET revision=$3 WHERE tenant_id=$1 AND series_id=$2").bind(req.tenant().as_str()).bind(series).bind(revision).execute(&mut *tx).await.map_err(|_|SummaryFailure::Storage)?;
        tx.commit().await.map_err(|_| SummaryFailure::Storage)?;
        Ok(SummaryReceipt { series, revision })
    }
}

async fn capture_restore(
    tx: &mut Transaction<'_, Postgres>,
    request: &NumericRequest,
) -> Result<(Option<Vec<u8>>, Vec<Value>), SummaryFailure> {
    let ticket = request.restore();
    let result = if let Some(ticket) = ticket {
        let state = sqlx::query_file!(
            "src/history/restore/history_snapshot.sql",
            request.tenant().as_str(),
            ticket.manifest(),
            ticket.generation()
        )
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| SummaryFailure::Storage)?
        .ok_or(SummaryFailure::CoverageUnavailable)?;
        if state.state != "restored" {
            return Err(SummaryFailure::CoverageUnavailable);
        }
        let file = state.payload.ok_or(SummaryFailure::CoverageUnavailable)?;
        let members:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_array(message_id,protocol_package_id,protocol_package_version,output_index,binding_id,trunc(binding_revision)::text,event_time_basis) FROM history.archive_members WHERE tenant_id=$1 AND job_id=$2").bind(request.tenant().as_str()).bind(ticket.manifest()).fetch_all(&mut **tx).await.map_err(|_|SummaryFailure::Storage)?;
        (Some(file), members)
    } else {
        (None, vec![])
    };
    Ok(result)
}

fn summary_body<D: SummaryArchiveDecoder>(
    snapshot: &mut NumericSnapshot,
    decoder: &D,
) -> Result<Value, SummaryFailure> {
    let req = &snapshot.request;
    let restored = match snapshot.file.as_deref() {
        Some(file) => decoder.decode(req.tenant(), file)?,
        None => vec![],
    };
    if restored.len() != snapshot.members.len()
        || snapshot
            .members
            .iter()
            .any(|id| !restored.iter().any(|c| key(c) == *id))
        || restored.iter().any(|c| c.tenant_id() != req.tenant())
    {
        return Err(SummaryFailure::CoverageUnavailable);
    }
    for row in restored.iter().filter(|c| {
        c.target() == req.target()
            && c.event_at().get() >= req.spec().start()
            && c.event_at().get() < req.spec().end()
    }) {
        if let Some(old) = snapshot.rows.iter().find(|r| r.id() == row.id()) {
            if old != row {
                return Err(SummaryFailure::InvalidEvidence);
            }
        } else {
            snapshot.rows.push(row.clone());
        }
    }
    let samples = snapshot
        .rows
        .iter()
        .map(|c| NumericSample {
            identity: key(c).to_string(),
            event_at_ms: c.event_at().get(),
            value: c.observation().value().clone(),
            unit: c.observation().source_unit().cloned(),
            quality: c.observation().quality(),
        })
        .collect::<Vec<_>>();
    let buckets = summarize(req.spec(), snapshot.kind, snapshot.unit.as_ref(), &samples)?;
    let mut inputs=samples.iter().map(|s|{
   let value=match &s.value{PropertyValue::Integer(v)=>v.to_string(),PropertyValue::Decimal(v)=>v.normalized().to_plain_string(),_=>unreachable!("validated numeric kind")};
   let quality=match s.quality {fleetiq_telemetry::ObservationQuality::Good=>"good",fleetiq_telemetry::ObservationQuality::Uncertain=>"uncertain",fleetiq_telemetry::ObservationQuality::Invalid=>"invalid"};
   json!({"id":s.identity,"event_at_ms":s.event_at_ms.to_string(),"value":value,"quality":quality})
  }).collect::<Vec<_>>();
    inputs.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    let ticket = req.restore();
    let body = json!({"algorithm":"numeric-fixed-v1","kind":if snapshot.kind==PropertyValueKind::Integer{"integer"}else{"decimal"},"unit":snapshot.unit.as_ref().map(UnitId::as_str),
   "coverage":{"scope":if ticket.is_some(){"hot_and_selected_manifest_snapshot"}else{"hot_snapshot"},"manifest_id":ticket.map(|t|t.manifest().to_string()),"restore_generation":ticket.map(|t|t.generation().to_string())},
   "inputs":inputs,"buckets":buckets.iter().map(|b|json!({"start_ms":b.start.to_string(),"end_ms":b.end.to_string(),"total":b.total,"count":b.count,"excluded":b.excluded,"min":b.minimum.as_ref().map(|v|v.normalized().to_plain_string()),"max":b.maximum.as_ref().map(|v|v.normalized().to_plain_string()),"sum":b.sum.normalized().to_plain_string()})).collect::<Vec<_>>()});
    if serde_json::to_vec(&body)
        .map_err(|_| SummaryFailure::Storage)?
        .len()
        > 1_048_576
    {
        return Err(SummaryFailure::InputLimit);
    }
    Ok(body)
}

mod read;

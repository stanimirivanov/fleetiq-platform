//! Single bounded statement reads one original publication without its private input rows.
use super::{
    NumericRequest, PostgresNumericSummaries, PropertyValueKind, RestoreTicket, SummaryFailure,
    SummaryReceipt, UnitId, Value,
};
use fleetiq_history::{
    downsampling::NumericBucket,
    summary_read::{NumericSummary, NumericSummaryView},
};
impl NumericSummaryView for PostgresNumericSummaries {
    async fn read_summary(
        &self,
        request: &NumericRequest,
        pinned: Option<SummaryReceipt>,
    ) -> Result<Option<NumericSummary>, SummaryFailure> {
        let mut tx = self.0.begin().await.map_err(|_| SummaryFailure::Storage)?;
        sqlx::raw_sql("SET TRANSACTION READ ONLY; SET LOCAL statement_timeout='2s';")
            .execute(&mut *tx)
            .await
            .map_err(|_| SummaryFailure::Storage)?;
        let target = request.target();
        let spec = request.spec();
        let ticket = request.restore();
        let row=sqlx::query!("SELECT d.series_id,d.revision,d.observed_at_ms,d.body-'inputs' AS \"body!\" FROM history.numeric_summary_series s JOIN history.numeric_summaries d USING(tenant_id,series_id) WHERE s.tenant_id=$1 AND s.asset_id=$2 AND s.property_id=$3 AND s.property_version=$4 AND s.start_ms=$5 AND s.end_ms=$6 AND s.width_ms=$7 AND s.algorithm=1 AND s.manifest_id IS NOT DISTINCT FROM $8 AND s.restore_generation IS NOT DISTINCT FROM $9 AND d.revision=COALESCE($11::bigint,s.revision) AND ($10::bigint IS NULL OR s.series_id=$10)",
   request.tenant().as_str(),target.asset_id().as_str(),target.property().id().as_str(),i64::from(target.property().version().get()),spec.start(),spec.end(),spec.width(),ticket.map(RestoreTicket::manifest),ticket.map(RestoreTicket::generation),pinned.map(|p|p.series),pinned.map(|p|p.revision))
   .fetch_optional(&mut *tx).await.map_err(|_|SummaryFailure::Storage)?;
        tx.commit().await.map_err(|_| SummaryFailure::Storage)?;
        row.map(|r| {
            let kind = match r.body["kind"].as_str() {
                Some("integer") => PropertyValueKind::Integer,
                Some("decimal") => PropertyValueKind::Decimal,
                _ => return Err(SummaryFailure::InvalidEvidence),
            };
            let unit = if r.body["unit"].is_null() {
                None
            } else {
                Some(
                    UnitId::new(
                        r.body["unit"]
                            .as_str()
                            .ok_or(SummaryFailure::InvalidEvidence)?,
                    )
                    .map_err(|_| SummaryFailure::InvalidEvidence)?,
                )
            };
            let buckets = r.body["buckets"]
                .as_array()
                .ok_or(SummaryFailure::InvalidEvidence)?
                .iter()
                .map(bucket)
                .collect::<Result<Vec<_>, _>>()?;
            if buckets.len()
                != usize::try_from((spec.end() - spec.start()) / spec.width())
                    .map_err(|_| SummaryFailure::InvalidEvidence)?
            {
                return Err(SummaryFailure::InvalidEvidence);
            }
            Ok(NumericSummary {
                receipt: SummaryReceipt {
                    series: r.series_id,
                    revision: r.revision,
                },
                observed_at_ms: r.observed_at_ms,
                kind,
                unit,
                buckets,
            })
        })
        .transpose()
    }
}
fn bucket(v: &Value) -> Result<NumericBucket, SummaryFailure> {
    let integer = |name: &str| {
        v[name]
            .as_str()
            .and_then(|s| s.parse().ok())
            .ok_or(SummaryFailure::InvalidEvidence)
    };
    let count = |name: &str| {
        v[name]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or(SummaryFailure::InvalidEvidence)
    };
    let number = |name: &str| {
        v[name]
            .as_str()
            .and_then(|s| s.parse::<bigdecimal::BigDecimal>().ok())
            .ok_or(SummaryFailure::InvalidEvidence)
    };
    Ok(NumericBucket {
        start: integer("start_ms")?,
        end: integer("end_ms")?,
        total: count("total")?,
        count: count("count")?,
        excluded: count("excluded")?,
        minimum: if v["min"].is_null() {
            None
        } else {
            Some(number("min")?)
        },
        maximum: if v["max"].is_null() {
            None
        } else {
            Some(number("max")?)
        },
        sum: number("sum")?,
    })
}

//! Version-one fixed numeric buckets; means are the exact sum/count pair, never rounded.
use bigdecimal::BigDecimal;
use fleetiq_domain::{PropertyValue, PropertyValueKind, UnitId};
use fleetiq_telemetry::ObservationQuality;
use std::collections::BTreeMap;
/// Explicit bounded failures; unavailable data never becomes a successful empty summary.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SummaryFailure {
    /// Unaligned, reversed, overflowing or excessive window/bucket specification.
    Bounds,
    /// Bounded source count, numeric envelope or persisted output was exceeded.
    InputLimit,
    /// Conflicting identities, units, kinds or invalid numeric context.
    InvalidEvidence,
    /// Missing/expired/incomplete selected restore or reclaimed hot evidence.
    CoverageUnavailable,
    /// A newer revision already contains evidence missing from this captured snapshot.
    StaleSnapshot,
    /// Lifecycle policy is disabled or tenant is inactive.
    Disabled,
    /// Tenant work allowance is unavailable or changed during production.
    BudgetDenied,
    /// Redacted persistence/decoder failure; retry the original request.
    Storage,
}
impl std::fmt::Display for SummaryFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "numeric summary rejected: {self:?}")
    }
}
impl std::error::Error for SummaryFailure {}
/// Epoch-aligned, half-open complete buckets; at most seven days and sixty-four buckets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NumericSpec {
    start: i64,
    end: i64,
    width: i64,
}
impl NumericSpec {
    /// Validates the complete window before any source read.
    /// # Errors
    /// Rejects invalid bounds, partial edge buckets and excessive bucket counts.
    pub fn new(start: i64, end: i64, width: i64) -> Result<Self, SummaryFailure> {
        let span = i128::from(end) - i128::from(start);
        if width <= 0
            || !(1..=604_800_000).contains(&span)
            || start.rem_euclid(width) != 0
            || end.rem_euclid(width) != 0
            || span / i128::from(width) > 64
        {
            return Err(SummaryFailure::Bounds);
        }
        Ok(Self { start, end, width })
    }
    /// Inclusive Unix-millisecond start.
    pub const fn start(self) -> i64 {
        self.start
    }
    /// Exclusive Unix-millisecond end.
    pub const fn end(self) -> i64 {
        self.end
    }
    /// Exact full bucket width in milliseconds.
    pub const fn width(self) -> i64 {
        self.width
    }
}
/// A scoped interpretation supplied by the trusted source adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NumericSample {
    /// Canonical complete interpretation key, not a lossy observation key.
    pub identity: String,
    /// Preserved selected event time.
    pub event_at_ms: i64,
    /// Original exact value; no unit conversion or numeric kind coercion.
    pub value: PropertyValue,
    /// Original canonical source unit.
    pub unit: Option<UnitId>,
    /// Only good quality contributes to numeric statistics.
    pub quality: ObservationQuality,
}
/// One exact derived bucket, including empty/excluded-only distinction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NumericBucket {
    /// Inclusive start.
    pub start: i64,
    /// Exclusive end.
    pub end: i64,
    /// Unique interpretation count, including excluded qualities.
    pub total: u32,
    /// Good-quality numeric count and denominator for the exact arithmetic mean.
    pub count: u32,
    /// Uncertain/invalid samples retained in lineage but excluded from arithmetic.
    pub excluded: u32,
    /// Minimum good value; absent when count is zero.
    pub minimum: Option<BigDecimal>,
    /// Maximum good value; absent when count is zero.
    pub maximum: Option<BigDecimal>,
    /// Exact sum and mean numerator; zero with count zero does not define a mean.
    pub sum: BigDecimal,
}
/// Reduces one bounded snapshot, deduplicating exact overlapping interpretations.
/// # Errors
/// Rejects malformed context, conflicting identities, unsupported kinds or resource excess.
pub fn summarize(
    spec: NumericSpec,
    kind: PropertyValueKind,
    unit: Option<&UnitId>,
    rows: &[NumericSample],
) -> Result<Vec<NumericBucket>, SummaryFailure> {
    if !matches!(
        kind,
        PropertyValueKind::Integer | PropertyValueKind::Decimal
    ) {
        return Err(SummaryFailure::InvalidEvidence);
    }
    if rows.len() > 116 {
        return Err(SummaryFailure::InputLimit);
    }
    let mut unique = BTreeMap::new();
    for row in rows {
        if row.identity.is_empty()
            || row.identity.len() > 8192
            || row.event_at_ms < spec.start
            || row.event_at_ms >= spec.end
            || row.unit.as_ref() != unit
            || row.value.kind() != kind
        {
            return Err(SummaryFailure::InvalidEvidence);
        }
        if let Some(old) = unique.insert(&row.identity, row)
            && old != row
        {
            return Err(SummaryFailure::InvalidEvidence);
        }
        if unique.len() > 100 {
            return Err(SummaryFailure::InputLimit);
        }
    }
    let n = (spec.end - spec.start) / spec.width;
    let mut buckets = (0..n)
        .map(|i| NumericBucket {
            start: spec.start + i * spec.width,
            end: spec.start + (i + 1) * spec.width,
            total: 0,
            count: 0,
            excluded: 0,
            minimum: None,
            maximum: None,
            sum: BigDecimal::from(0),
        })
        .collect::<Vec<_>>();
    for row in unique.values() {
        let value = match &row.value {
            PropertyValue::Integer(v) => BigDecimal::from(*v),
            PropertyValue::Decimal(v) => v.clone(),
            _ => return Err(SummaryFailure::InvalidEvidence),
        };
        // Reuse the existing calculator numeric envelope before aligning decimal scales.
        let (coefficient, scale) = value.as_bigint_and_exponent();
        if !(-256..=256).contains(&scale) || coefficient.to_string().len() > 1024 {
            return Err(SummaryFailure::InputLimit);
        }
        let index = usize::try_from((row.event_at_ms - spec.start) / spec.width)
            .map_err(|_| SummaryFailure::Bounds)?;
        let bucket = &mut buckets[index];
        bucket.total += 1;
        if row.quality != ObservationQuality::Good {
            bucket.excluded += 1;
            continue;
        }
        bucket.count += 1;
        bucket.sum += &value;
        if bucket.minimum.as_ref().is_none_or(|v| &value < v) {
            bucket.minimum = Some(value.clone());
        }
        if bucket.maximum.as_ref().is_none_or(|v| &value > v) {
            bucket.maximum = Some(value);
        }
    }
    Ok(buckets)
}

/// Explicit trusted producer request; no automatic cross-tier scan or side effect.
#[derive(Clone)]
pub struct NumericRequest {
    tenant: fleetiq_domain::TenantId,
    target: fleetiq_domain::SignalTarget,
    spec: NumericSpec,
    restore: Option<crate::restore::RestoreTicket>,
}
impl NumericRequest {
    /// Combines validated scope/window with an optional exact restore generation.
    pub const fn new(
        tenant: fleetiq_domain::TenantId,
        target: fleetiq_domain::SignalTarget,
        spec: NumericSpec,
        restore: Option<crate::restore::RestoreTicket>,
    ) -> Self {
        Self {
            tenant,
            target,
            spec,
            restore,
        }
    }
    /// Tenant owning the snapshot and summary revisions.
    pub const fn tenant(&self) -> &fleetiq_domain::TenantId {
        &self.tenant
    }
    /// Exact asset/property definition version.
    pub const fn target(&self) -> &fleetiq_domain::SignalTarget {
        &self.target
    }
    /// Fixed bucket/window contract.
    pub const fn spec(&self) -> NumericSpec {
        self.spec
    }
    /// Explicit selected manifest/request generation, if any.
    pub const fn restore(&self) -> Option<crate::restore::RestoreTicket> {
        self.restore
    }
}
/// Original immutable publication identity, reused for unchanged evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SummaryReceipt {
    /// Tenant-scoped series identity (target/window/resolution/selection/algorithm).
    pub series: i64,
    /// Monotone publication revision within the series, not an arrival watermark.
    pub revision: i64,
}

/// Trusted bounded codec port; vendor/file representations stay outside history.
pub trait SummaryArchiveDecoder: Send + Sync {
    /// Decodes exactly the captured verified file, preserving every original interpretation.
    /// # Errors
    /// Rejects malformed, oversized, unsupported or cross-tenant archive evidence.
    fn decode(
        &self,
        tenant: &fleetiq_domain::TenantId,
        bytes: &[u8],
    ) -> Result<Vec<crate::StoredPropertyChange>, SummaryFailure>;
}

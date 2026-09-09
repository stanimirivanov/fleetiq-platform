use std::{error::Error, fmt, num::NonZeroU64};

use crate::{AssetId, AssetRelationshipId, RelationshipTypeRef, TenantId};

/// A signed Unix timestamp with millisecond precision.
///
/// The domain uses an explicit representation so interval semantics do not
/// depend on a database, serialization format, or clock library. Negative values
/// represent instants before the Unix epoch.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UnixTimeMillis(i64);

impl UnixTimeMillis {
    /// Constructs a timestamp from milliseconds relative to the Unix epoch.
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    /// Returns the number of milliseconds relative to the Unix epoch.
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// A positive revision of a recorded domain fact.
///
/// Revisions preserve the order in which `FleetIQ` learned or corrected facts;
/// they are independent of the interval in which those facts applied physically.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RecordRevision(NonZeroU64);

impl RecordRevision {
    /// Constructs a record revision.
    ///
    /// # Errors
    ///
    /// Returns [`RecordRevisionError`] when `value` is zero. Revision numbering
    /// starts at one.
    pub fn new(value: u64) -> Result<Self, RecordRevisionError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(RecordRevisionError::Zero)
    }

    /// Returns the numeric revision.
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

/// Describes why a record revision could not be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordRevisionError {
    /// Revision zero is reserved as an invalid/uninitialized value.
    Zero,
}

impl fmt::Display for RecordRevisionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("record revision must be greater than zero")
    }
}

impl Error for RecordRevisionError {}

/// Identifies a revision and when it was recorded by `FleetIQ`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RecordedRevision {
    number: RecordRevision,
    at: UnixTimeMillis,
}

impl RecordedRevision {
    /// Constructs recorded-revision metadata.
    pub const fn new(number: RecordRevision, at: UnixTimeMillis) -> Self {
        Self { number, at }
    }

    /// Returns the monotonically increasing record revision.
    pub const fn number(self) -> RecordRevision {
        self.number
    }

    /// Returns when `FleetIQ` recorded the revision.
    pub const fn at(self) -> UnixTimeMillis {
        self.at
    }
}

/// A half-open interval in which a domain fact applies: `[start, end)`.
///
/// An absent end represents an interval that remains effective indefinitely.
/// The exclusive end lets adjacent intervals meet without overlapping.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EffectiveInterval {
    start: UnixTimeMillis,
    end: Option<UnixTimeMillis>,
}

impl EffectiveInterval {
    /// Constructs an effective interval.
    ///
    /// # Errors
    ///
    /// Returns [`EffectiveIntervalError::EndNotAfterStart`] when a supplied end
    /// is earlier than or equal to the start.
    pub fn new(
        start: UnixTimeMillis,
        end: Option<UnixTimeMillis>,
    ) -> Result<Self, EffectiveIntervalError> {
        if end.is_some_and(|candidate| candidate <= start) {
            return Err(EffectiveIntervalError::EndNotAfterStart);
        }

        Ok(Self { start, end })
    }

    /// Returns the inclusive start of the interval.
    pub fn start(self) -> UnixTimeMillis {
        self.start
    }

    /// Returns the exclusive end, or `None` when the interval is open-ended.
    pub fn end(self) -> Option<UnixTimeMillis> {
        self.end
    }

    /// Reports whether an instant belongs to the half-open interval.
    pub fn contains(self, instant: UnixTimeMillis) -> bool {
        instant >= self.start && self.end.is_none_or(|end| instant < end)
    }

    /// Reports whether this interval overlaps another interval.
    pub fn overlaps(self, other: Self) -> bool {
        self.end.is_none_or(|end| other.start < end) && other.end.is_none_or(|end| self.start < end)
    }
}

/// Describes why an effective interval could not be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectiveIntervalError {
    /// The exclusive end is earlier than or equal to the inclusive start.
    EndNotAfterStart,
}

impl fmt::Display for EffectiveIntervalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("effective interval end must be later than its start")
    }
}

impl Error for EffectiveIntervalError {}

/// The ordered source and target of a directed asset relationship.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RelationshipEndpoints {
    source_asset_id: AssetId,
    target_asset_id: AssetId,
}

impl RelationshipEndpoints {
    /// Constructs directed relationship endpoints.
    pub fn new(source_asset_id: AssetId, target_asset_id: AssetId) -> Self {
        Self {
            source_asset_id,
            target_asset_id,
        }
    }

    /// Returns the asset at the directed source of the relationship.
    pub fn source_asset_id(&self) -> &AssetId {
        &self.source_asset_id
    }

    /// Returns the asset at the directed target of the relationship.
    pub fn target_asset_id(&self) -> &AssetId {
        &self.target_asset_id
    }
}

/// A recorded revision of a directed relationship between two assets.
///
/// The source and target roles are meaningful according to the referenced
/// relationship type. Both assets must belong to [`Self::tenant_id`]; enforcing
/// that rule requires an asset repository and is therefore an application-layer
/// validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetRelationship {
    tenant_id: TenantId,
    id: AssetRelationshipId,
    recorded_revision: RecordedRevision,
    type_reference: RelationshipTypeRef,
    endpoints: RelationshipEndpoints,
    effective_interval: EffectiveInterval,
}

impl AssetRelationship {
    /// Constructs a recorded relationship revision.
    pub fn new(
        tenant_id: TenantId,
        id: AssetRelationshipId,
        recorded_revision: RecordedRevision,
        type_reference: RelationshipTypeRef,
        endpoints: RelationshipEndpoints,
        effective_interval: EffectiveInterval,
    ) -> Self {
        Self {
            tenant_id,
            id,
            recorded_revision,
            type_reference,
            endpoints,
            effective_interval,
        }
    }

    /// Returns the tenant whose graph contains this relationship.
    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the stable relationship identity shared by its revisions.
    pub fn id(&self) -> &AssetRelationshipId {
        &self.id
    }

    /// Returns this recorded fact's revision.
    pub fn revision(&self) -> RecordRevision {
        self.recorded_revision.number()
    }

    /// Returns when `FleetIQ` recorded this revision.
    pub fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_revision.at()
    }

    /// Returns the relationship type and version that define edge semantics.
    pub fn relationship_type(&self) -> &RelationshipTypeRef {
        &self.type_reference
    }

    /// Returns the asset at the directed source of the relationship.
    pub fn source_asset_id(&self) -> &AssetId {
        self.endpoints.source_asset_id()
    }

    /// Returns the asset at the directed target of the relationship.
    pub fn target_asset_id(&self) -> &AssetId {
        self.endpoints.target_asset_id()
    }

    /// Returns the interval in which this relationship applies physically.
    pub fn effective_interval(&self) -> EffectiveInterval {
        self.effective_interval
    }

    /// Reports whether the relationship applies at an effective-time instant.
    pub fn is_effective_at(&self, instant: UnixTimeMillis) -> bool {
        self.effective_interval.contains(instant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DefinitionVersion, RelationshipTypeId};

    fn instant(value: i64) -> UnixTimeMillis {
        UnixTimeMillis::new(value)
    }

    fn relationship(interval: EffectiveInterval) -> AssetRelationship {
        AssetRelationship::new(
            TenantId::new("tenant-a").expect("valid tenant ID"),
            AssetRelationshipId::new("relationship-1").expect("valid relationship ID"),
            RecordedRevision::new(
                RecordRevision::new(1).expect("valid record revision"),
                instant(2_000),
            ),
            RelationshipTypeRef::new(
                RelationshipTypeId::new("physical.contains").expect("valid relationship type ID"),
                DefinitionVersion::new(1).expect("valid definition version"),
            ),
            RelationshipEndpoints::new(
                AssetId::new("assembly-a").expect("valid source asset ID"),
                AssetId::new("component-b").expect("valid target asset ID"),
            ),
            interval,
        )
    }

    #[test]
    fn revisions_start_at_one() {
        assert_eq!(RecordRevision::new(0), Err(RecordRevisionError::Zero));
        assert_eq!(
            RecordRevision::new(1).expect("valid record revision").get(),
            1
        );
    }

    #[test]
    fn rejects_empty_and_reversed_intervals() {
        assert_eq!(
            EffectiveInterval::new(instant(10), Some(instant(10))),
            Err(EffectiveIntervalError::EndNotAfterStart)
        );
        assert_eq!(
            EffectiveInterval::new(instant(10), Some(instant(9))),
            Err(EffectiveIntervalError::EndNotAfterStart)
        );
    }

    #[test]
    fn interval_is_inclusive_at_start_and_exclusive_at_end() {
        let interval = EffectiveInterval::new(instant(10), Some(instant(20)))
            .expect("valid effective interval");

        assert!(!interval.contains(instant(9)));
        assert!(interval.contains(instant(10)));
        assert!(interval.contains(instant(19)));
        assert!(!interval.contains(instant(20)));
    }

    #[test]
    fn open_interval_has_no_effective_end() {
        let interval =
            EffectiveInterval::new(instant(10), None).expect("valid open effective interval");

        assert_eq!(interval.start(), instant(10));
        assert_eq!(interval.end(), None);
        assert!(interval.contains(instant(i64::MAX)));
    }

    #[test]
    fn adjacent_intervals_do_not_overlap() {
        let first = EffectiveInterval::new(instant(10), Some(instant(20)))
            .expect("valid effective interval");
        let second = EffectiveInterval::new(instant(20), Some(instant(30)))
            .expect("valid effective interval");
        let overlapping = EffectiveInterval::new(instant(19), Some(instant(30)))
            .expect("valid effective interval");

        assert!(!first.overlaps(second));
        assert!(first.overlaps(overlapping));
    }

    #[test]
    fn relationship_preserves_direction_type_and_bitemporal_metadata() {
        let interval = EffectiveInterval::new(instant(1_000), Some(instant(3_000)))
            .expect("valid effective interval");
        let relationship = relationship(interval);

        assert_eq!(relationship.tenant_id().as_str(), "tenant-a");
        assert_eq!(relationship.id().as_str(), "relationship-1");
        assert_eq!(relationship.revision().get(), 1);
        assert_eq!(relationship.recorded_at(), instant(2_000));
        assert_eq!(
            relationship.relationship_type().id().as_str(),
            "physical.contains"
        );
        assert_eq!(relationship.source_asset_id().as_str(), "assembly-a");
        assert_eq!(relationship.target_asset_id().as_str(), "component-b");
        assert_eq!(relationship.effective_interval(), interval);
        assert!(relationship.is_effective_at(instant(1_500)));
    }
}

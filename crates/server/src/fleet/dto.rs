//! Wire strings preserve exact coordinates, counters, timestamps and provenance.
use crate::property_history::dto::ChangeBody;
use fleetiq_domain::{
    AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, UnixTimeMillis,
};
use fleetiq_projection::{
    CoordinateEncoding, FleetQuery, FleetSnapshot, Freshness, LocationMapping, LocationState,
    classify_location,
};
use fleetiq_telemetry::ObservationQuality;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Parameters {
    latitude_property: String,
    latitude_version: u32,
    longitude_property: String,
    longitude_version: u32,
    encoding: String,
    limit: Option<u16>,
    after: Option<String>,
}
impl Parameters {
    pub(super) fn query(self) -> Option<FleetQuery> {
        let property = |id, version| {
            Some(PropertyDefinitionRef::new(
                PropertyDefinitionId::new(id).ok()?,
                DefinitionVersion::new(version).ok()?,
            ))
        };
        let encoding = match self.encoding.as_str() {
            "degrees" => CoordinateEncoding::Degrees,
            "e7" => CoordinateEncoding::E7,
            _ => return None,
        };
        let mapping = LocationMapping::new(
            property(self.latitude_property, self.latitude_version)?,
            property(self.longitude_property, self.longitude_version)?,
            encoding,
        )
        .ok()?;
        FleetQuery::new(
            mapping,
            self.limit.unwrap_or(50),
            self.after.map(AssetId::new).transpose().ok()?,
        )
        .ok()
    }
}
#[derive(Serialize)]
pub(super) struct Body {
    generation: String,
    revision: String,
    pending: bool,
    evaluated_at_ms: String,
    stale_after_ms: u32,
    mapping: MappingBody,
    mapping_supported: bool,
    summary: SummaryBody,
    assets: Vec<AssetBody>,
    next_after: Option<String>,
}
#[derive(Serialize)]
struct MappingBody {
    latitude: DefinitionBody,
    longitude: DefinitionBody,
    encoding: &'static str,
}
#[derive(Serialize)]
struct DefinitionBody {
    id: String,
    version: u32,
}
impl From<&PropertyDefinitionRef> for DefinitionBody {
    fn from(p: &PropertyDefinitionRef) -> Self {
        Self {
            id: p.id().as_str().to_owned(),
            version: p.version().get(),
        }
    }
}
#[derive(Serialize)]
struct SummaryBody {
    assets: String,
    communication_devices: String,
    mapped_assets: String,
    assets_with_current_properties: String,
}
#[derive(Serialize)]
struct AssetBody {
    asset_id: String,
    display_name: String,
    asset_type: DefinitionBody,
    location: PositionBody,
}
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum PositionBody {
    Unmapped,
    IncompatibleMapping,
    Missing,
    Incoherent,
    InvalidCoordinates,
    InvalidQuality,
    Observed { point: Box<PointBody> },
}
#[derive(Serialize)]
struct PointBody {
    latitude: String,
    longitude: String,
    quality: &'static str,
    freshness: &'static str,
    latitude_change: ChangeBody,
    longitude_change: ChangeBody,
}
impl Body {
    pub(super) fn new(
        snapshot: &FleetSnapshot,
        mapping: &LocationMapping,
    ) -> Result<Self, serde_json::Error> {
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|t| i64::try_from(t.as_millis()).ok())
            .ok_or_else(|| {
                <serde_json::Error as serde::ser::Error>::custom("evaluation time unavailable")
            })?;
        Self::at(snapshot, mapping, UnixTimeMillis::new(at))
    }
    pub(super) fn at(
        snapshot: &FleetSnapshot,
        mapping: &LocationMapping,
        at: UnixTimeMillis,
    ) -> Result<Self, serde_json::Error> {
        let assets = snapshot
            .assets
            .iter()
            .map(|asset| {
                let location =
                    match classify_location(asset, mapping, snapshot.mapping_supported, at) {
                        LocationState::Unmapped => PositionBody::Unmapped,
                        LocationState::IncompatibleMapping => PositionBody::IncompatibleMapping,
                        LocationState::Missing => PositionBody::Missing,
                        LocationState::Incoherent => PositionBody::Incoherent,
                        LocationState::InvalidCoordinates => PositionBody::InvalidCoordinates,
                        LocationState::InvalidQuality => PositionBody::InvalidQuality,
                        LocationState::Observed(point) => {
                            let (lat, lon) = asset
                                .latitude
                                .as_ref()
                                .zip(asset.longitude.as_ref())
                                .ok_or_else(|| {
                                    <serde_json::Error as serde::ser::Error>::custom(
                                        "coherent point has no provenance",
                                    )
                                })?;
                            PositionBody::Observed {
                                point: Box::new(PointBody {
                                    latitude: point.latitude,
                                    longitude: point.longitude,
                                    quality: match point.quality {
                                        ObservationQuality::Good => "good",
                                        ObservationQuality::Uncertain => "uncertain",
                                        ObservationQuality::Invalid => "invalid",
                                    },
                                    freshness: match point.freshness {
                                        Freshness::Fresh => "fresh",
                                        Freshness::Stale => "stale",
                                        Freshness::Future => "future",
                                    },
                                    latitude_change: ChangeBody::new(lat)?,
                                    longitude_change: ChangeBody::new(lon)?,
                                }),
                            }
                        }
                    };
                Ok(AssetBody {
                    asset_id: asset.id.as_str().to_owned(),
                    display_name: asset.name.as_str().to_owned(),
                    asset_type: DefinitionBody {
                        id: asset.asset_type.id().as_str().to_owned(),
                        version: asset.asset_type.version().get(),
                    },
                    location,
                })
            })
            .collect::<Result<Vec<_>, serde_json::Error>>()?;
        Ok(Self {
            generation: snapshot.generation.to_string(),
            revision: snapshot.revision.to_string(),
            pending: snapshot.pending,
            evaluated_at_ms: at.get().to_string(),
            stale_after_ms: 30000,
            mapping: MappingBody {
                latitude: mapping.latitude().into(),
                longitude: mapping.longitude().into(),
                encoding: match mapping.encoding() {
                    CoordinateEncoding::Degrees => "degrees",
                    CoordinateEncoding::E7 => "e7",
                },
            },
            mapping_supported: snapshot.mapping_supported,
            summary: SummaryBody {
                assets: snapshot.summary.assets.to_string(),
                communication_devices: snapshot.summary.communication_devices.to_string(),
                mapped_assets: snapshot.summary.mapped_assets.to_string(),
                assets_with_current_properties: snapshot
                    .summary
                    .assets_with_current_properties
                    .to_string(),
            },
            assets,
            next_after: snapshot
                .next_after
                .as_ref()
                .map(|id| id.as_str().to_owned()),
        })
    }
}

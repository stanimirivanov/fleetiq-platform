use fleetiq_domain::AssetId;
use fleetiq_ingestion::{
    ProtocolAdmissionWorld, ProtocolChannelId, ProtocolDeviceRoute, ProtocolRouteSelection,
};
use fleetiq_telemetry::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Deserialize, Serialize)]
enum World {
    #[serde(rename = "runtime-v1")]
    Runtime,
    #[serde(rename = "identity-v1")]
    Identity,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    package_id: String,
    package_version: String,
    world: World,
}
// A newtype keeps selection required on the wire while accepting explicit null.
// A bare Option field would silently interpret omission as retirement.
#[derive(Deserialize)]
struct RequiredSelection(Option<Selection>);
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ChangeBody {
    pub expected_revision: u32,
    selection: RequiredSelection,
}
impl ChangeBody {
    pub(super) fn selection(self) -> Result<Option<ProtocolRouteSelection>, ()> {
        self.selection
            .0
            .map(|selection| {
                Ok(ProtocolRouteSelection {
                    package: ProtocolPackageRef::new(
                        ProtocolPackageId::new(selection.package_id).map_err(|_| ())?,
                        ProtocolPackageVersion::new(selection.package_version).map_err(|_| ())?,
                    ),
                    world: match selection.world {
                        World::Runtime => ProtocolAdmissionWorld::Runtime,
                        World::Identity => ProtocolAdmissionWorld::Identity,
                    },
                })
            })
            .transpose()
    }
}
pub(super) fn identity(device: &str, channel: &str) -> Option<(AssetId, ProtocolChannelId)> {
    Some((
        AssetId::new(device).ok()?,
        ProtocolChannelId::new(channel).ok()?,
    ))
}
#[derive(Serialize)]
pub(super) struct RouteBody {
    device_asset_id: String,
    channel_id: String,
    revision: u32,
    selection: Option<Selection>,
    policy_version: Option<i32>,
    actor_id: String,
    recorded_at_ms: i64,
}
impl From<&ProtocolDeviceRoute> for RouteBody {
    fn from(route: &ProtocolDeviceRoute) -> Self {
        Self {
            device_asset_id: route.device_asset_id.to_string(),
            channel_id: route.channel_id.as_str().into(),
            revision: route.revision.get(),
            selection: route.selection.as_ref().map(|selection| Selection {
                package_id: selection.package.id().to_string(),
                package_version: selection.package.version().to_string(),
                world: match selection.world {
                    ProtocolAdmissionWorld::Runtime => World::Runtime,
                    ProtocolAdmissionWorld::Identity => World::Identity,
                },
            }),
            policy_version: route.policy_version,
            actor_id: route.actor_id.to_string(),
            recorded_at_ms: route.recorded_at.get(),
        }
    }
}

//! Serialized commissioning and current routing snapshots for ingestion.
mod read;
mod write;
use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};
use fleetiq_domain::{ActorId, AssetId, TenantId, UnixTimeMillis};
use fleetiq_ingestion::{
    PROTOCOL_ADMISSION_POLICY, ProtocolAdmissionWorld, ProtocolChannelId, ProtocolDeviceRoute,
    ProtocolPackageRelease, ProtocolRouteCommand, ProtocolRouteSelection, ProtocolRouteWrite,
    ProtocolRoutingRepository, ProtocolSessionRoute, RegisteredIdentityChannel,
};
use fleetiq_protocol_sdk::DeviceIdentityClaim;
use fleetiq_telemetry::{
    ProtocolArtifactDigest, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};
use sqlx::{PgConnection, PgPool};
use std::num::{NonZeroU32, NonZeroU64};

/// PostgreSQL owns route generations; no listener or guest authentication is implied.
#[derive(Clone, Debug)]
pub struct PostgresProtocolRoutingRepository {
    pool: PgPool,
}
impl PostgresProtocolRoutingRepository {
    /// Uses the application pool; immutable package storage is reused.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
impl ProtocolRoutingRepository for PostgresProtocolRoutingRepository {
    type Error = PostgresRepositoryError;
    async fn find(
        &self,
        tenant: &TenantId,
        device: &AssetId,
        channel: &ProtocolChannelId,
    ) -> Result<Option<ProtocolDeviceRoute>, Self::Error> {
        let mut connection = self.pool.acquire().await.map_err(classify_database_error)?;
        read::find(&mut connection, tenant, device, channel, None).await
    }
    async fn change(
        &self,
        command: &ProtocolRouteCommand,
    ) -> Result<ProtocolRouteWrite, Self::Error> {
        self.replace(command).await
    }
    async fn resolve(
        &self,
        channel: &RegisteredIdentityChannel,
        claim: &DeviceIdentityClaim,
    ) -> Result<Option<ProtocolSessionRoute>, Self::Error> {
        self.lookup(channel, claim).await
    }
}
struct RouteRow {
    tenant_id: String,
    device_asset_id: String,
    channel_id: String,
    revision: i64,
    actor_id: String,
    recorded_at_ms: i64,
    protocol_package_id: Option<String>,
    protocol_package_version: Option<String>,
    wit_world: Option<String>,
    policy_version: Option<i32>,
    artifact_sha256: Option<Vec<u8>>,
    artifact_size_bytes: Option<bigdecimal::BigDecimal>,
}
impl RouteRow {
    fn route(&self) -> Result<ProtocolDeviceRoute, PostgresRepositoryError> {
        let invalid = || corrupt("protocol_route_revisions", "invalid configuration");
        let selection = match (
            &self.protocol_package_id,
            &self.protocol_package_version,
            &self.wit_world,
            self.policy_version,
        ) {
            (None, None, None, None) => None,
            (Some(id), Some(version), Some(world), Some(policy)) if policy > 0 => {
                Some(ProtocolRouteSelection {
                    package: ProtocolPackageRef::new(
                        ProtocolPackageId::new(id.clone()).map_err(|_| invalid())?,
                        ProtocolPackageVersion::new(version.clone()).map_err(|_| invalid())?,
                    ),
                    world: match world.as_str() {
                        "runtime-v1" => ProtocolAdmissionWorld::Runtime,
                        "identity-v1" => ProtocolAdmissionWorld::Identity,
                        _ => return Err(invalid()),
                    },
                })
            }
            _ => return Err(invalid()),
        };
        Ok(ProtocolDeviceRoute {
            tenant_id: TenantId::new(self.tenant_id.clone()).map_err(|_| invalid())?,
            device_asset_id: AssetId::new(self.device_asset_id.clone()).map_err(|_| invalid())?,
            channel_id: ProtocolChannelId::new(self.channel_id.clone()).map_err(|_| invalid())?,
            revision: u32::try_from(self.revision)
                .ok()
                .and_then(NonZeroU32::new)
                .ok_or_else(invalid)?,
            selection,
            policy_version: self.policy_version,
            actor_id: ActorId::new(self.actor_id.clone()).map_err(|_| invalid())?,
            recorded_at: UnixTimeMillis::new(self.recorded_at_ms),
        })
    }
    fn session(self) -> Result<ProtocolSessionRoute, PostgresRepositoryError> {
        let route = self.route()?;
        let package = route
            .selection
            .as_ref()
            .ok_or_else(|| corrupt("protocol_routes", "retired session"))?
            .package
            .clone();
        let digest: [u8; 32] = self
            .artifact_sha256
            .ok_or_else(|| corrupt("protocol_routes", "missing digest"))?
            .try_into()
            .map_err(|_| corrupt("protocol_routes", "invalid digest"))?;
        let size = self
            .artifact_size_bytes
            .and_then(|value| value.to_string().parse::<u64>().ok())
            .and_then(NonZeroU64::new)
            .ok_or_else(|| corrupt("protocol_routes", "invalid length"))?;
        let release = ProtocolPackageRelease::new(
            route.tenant_id.clone(),
            package,
            ProtocolArtifactDigest::from_sha256(digest),
            size,
        );
        Ok(ProtocolSessionRoute { route, release })
    }
}

mod live;

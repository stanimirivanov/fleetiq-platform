use super::{
    AssetId, DeviceIdentityClaim, PROTOCOL_ADMISSION_POLICY, PgConnection,
    PostgresProtocolRoutingRepository, PostgresRepositoryError, ProtocolChannelId,
    ProtocolDeviceRoute, ProtocolSessionRoute, RegisteredIdentityChannel, RouteRow, TenantId,
    classify_database_error,
};
pub(super) async fn find(
    connection: &mut PgConnection,
    tenant: &TenantId,
    device: &AssetId,
    channel: &ProtocolChannelId,
    generation: Option<i64>,
) -> Result<Option<ProtocolDeviceRoute>, PostgresRepositoryError> {
    sqlx::query_as!(RouteRow,r"SELECT revision.tenant_id,revision.device_asset_id,revision.channel_id,revision.revision,revision.actor_id,revision.recorded_at_ms,
       revision.protocol_package_id,revision.protocol_package_version,revision.wit_world,revision.policy_version,
       package.artifact_sha256,package.artifact_size_bytes
FROM ingestion.protocol_route_heads AS head
JOIN ingestion.protocol_route_revisions AS revision ON revision.tenant_id=head.tenant_id AND revision.device_asset_id=head.device_asset_id AND revision.channel_id=head.channel_id AND revision.revision=COALESCE($4,head.revision)
LEFT JOIN metadata.protocol_package_releases AS package ON package.tenant_id=revision.tenant_id
 AND package.protocol_package_id=revision.protocol_package_id AND package.protocol_package_version=revision.protocol_package_version
WHERE head.tenant_id=$1 AND head.device_asset_id=$2 AND head.channel_id=$3",
        tenant.as_str(),device.as_str(),channel.as_str(),generation).fetch_optional(connection).await.map_err(classify_database_error)?
        .map(|row|row.route()).transpose()
}
impl PostgresProtocolRoutingRepository {
    pub(super) async fn lookup(
        &self,
        channel: &RegisteredIdentityChannel,
        claim: &DeviceIdentityClaim,
    ) -> Result<Option<ProtocolSessionRoute>, PostgresRepositoryError> {
        let world = channel.selection.world.as_str();
        let authority = channel
            .authority
            .as_ref()
            .map(fleetiq_domain::ExternalIdentifierAuthority::as_str);
        // One READ COMMITTED statement is the eligibility linearization point.
        // The registered identifier's existing unique key bounds this to one
        // device. Tenant, issuer, channel and decoder are always host-selected.
        sqlx::query_as!(RouteRow,r"SELECT revision.tenant_id,revision.device_asset_id,revision.channel_id,revision.revision,revision.actor_id,revision.recorded_at_ms,
       revision.protocol_package_id,revision.protocol_package_version,revision.wit_world,revision.policy_version,
       package.artifact_sha256,package.artifact_size_bytes
FROM ingestion.protocol_route_heads AS head
JOIN ingestion.protocol_route_revisions AS revision USING (tenant_id,device_asset_id,channel_id,revision)
LEFT JOIN metadata.protocol_package_releases AS package ON package.tenant_id=revision.tenant_id
 AND package.protocol_package_id=revision.protocol_package_id AND package.protocol_package_version=revision.protocol_package_version
JOIN metadata.tenants AS tenant ON tenant.tenant_id=head.tenant_id AND tenant.is_active
JOIN metadata.asset_external_identifiers AS identifier ON identifier.tenant_id=head.tenant_id AND identifier.asset_id=head.device_asset_id
JOIN ingestion.protocol_admissions AS admission ON admission.tenant_id=revision.tenant_id
 AND admission.protocol_package_id=revision.protocol_package_id AND admission.protocol_package_version=revision.protocol_package_version
 AND admission.wit_world=revision.wit_world AND admission.policy_version=revision.policy_version
JOIN ingestion.protocol_artifacts AS artifact ON artifact.tenant_id=package.tenant_id AND artifact.artifact_sha256=package.artifact_sha256
WHERE head.tenant_id=$1 AND head.channel_id=$2 AND identifier.identifier_kind=$3 AND identifier.authority IS NOT DISTINCT FROM $4
 AND identifier.identifier_value=$5 AND revision.protocol_package_id=$6 AND revision.protocol_package_version=$7
 AND revision.wit_world=$8 AND revision.policy_version=$9 AND octet_length(artifact.artifact_bytes)=package.artifact_size_bytes",
            channel.tenant_id.as_str(),channel.channel_id.as_str(),claim.kind(),authority,claim.value(),
            channel.selection.package.id().as_str(),channel.selection.package.version().as_str(),world,PROTOCOL_ADMISSION_POLICY)
            .fetch_optional(&self.pool).await.map_err(classify_database_error)?.map(RouteRow::session).transpose()
    }
}

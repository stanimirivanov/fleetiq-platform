//! Exact pre-identity admission snapshot; this does not authenticate a peer.
use super::{
    NonZeroU64, PROTOCOL_ADMISSION_POLICY, PostgresProtocolRoutingRepository,
    ProtocolAdmissionWorld, ProtocolArtifactDigest, ProtocolPackageRelease,
    RegisteredIdentityChannel, classify_database_error, corrupt,
};

impl fleetiq_ingestion::LiveRoutingRepository for PostgresProtocolRoutingRepository {
    async fn bootstrap(
        &self,
        channel: &RegisteredIdentityChannel,
    ) -> Result<Option<ProtocolPackageRelease>, Self::Error> {
        if channel.selection.world != ProtocolAdmissionWorld::Identity {
            return Ok(None);
        }
        // One statement is the pre-identity execution eligibility snapshot.
        // It is tenant/release scoped, checks current admission and byte length,
        // and returns only content metadata; no peer is trusted by this lookup.
        let row = sqlx::query!(
            r#"
            SELECT package.artifact_sha256,
            package.artifact_size_bytes::bigint AS "size!"
            -- The release, active tenant, admission and stored content are read
            -- from one statement snapshot before executing untrusted identity code.
            FROM metadata.protocol_package_releases package
            JOIN metadata.tenants tenant USING(tenant_id)
            JOIN ingestion.protocol_admissions admission
                USING (tenant_id, protocol_package_id, protocol_package_version, artifact_sha256)
            JOIN ingestion.protocol_artifacts artifact USING (tenant_id, artifact_sha256)
            WHERE package.tenant_id = $1
              AND package.protocol_package_id = $2
              AND package.protocol_package_version = $3
            AND tenant.is_active
              AND admission.wit_world = 'identity-v1'
              AND admission.policy_version = $4
            -- Length is an inexpensive corruption guard; the content adapter also
              -- checks the digest before the child receives artifact bytes.
              AND octet_length(artifact.artifact_bytes) = package.artifact_size_bytes"#,
            channel.tenant_id.as_str(),
            channel.selection.package.id().as_str(),
            channel.selection.package.version().as_str(),
            PROTOCOL_ADMISSION_POLICY
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?;
        row.map(|row| {
            let digest = row
                .artifact_sha256
                .try_into()
                .map_err(|_| corrupt("protocol_channels", "invalid digest"))?;
            let size = u64::try_from(row.size)
                .ok()
                .and_then(NonZeroU64::new)
                .ok_or_else(|| corrupt("protocol_channels", "invalid size"))?;
            Ok(ProtocolPackageRelease::new(
                channel.tenant_id.clone(),
                channel.selection.package.clone(),
                ProtocolArtifactDigest::from_sha256(digest),
                size,
            ))
        })
        .transpose()
    }
}

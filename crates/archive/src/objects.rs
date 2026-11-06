//! Trusted immutable S3 boundary; keys never come from request-controlled paths.
mod transport;
use crate::{MAX_FILE_BYTES, decode};
use bytes::Bytes;
use fleetiq_domain::TenantId;
use object_store::{
    GetOptions, ObjectStore, ObjectStoreExt, PutMode, RetryConfig,
    aws::{AmazonS3, AmazonS3Builder, S3ConditionalPut},
    path::Path,
};
use sha2::{Digest, Sha256};
use std::{fmt, time::Duration};
use url::Url;
/// Redacted operator configuration, independent of ambient AWS discovery.
#[derive(Clone)]
pub struct S3ArchiveConfig {
    endpoint: Url,
    bucket: String,
    region: String,
    access: String,
    secret: String,
}
impl fmt::Debug for S3ArchiveConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("S3ArchiveConfig([REDACTED])")
    }
}
impl S3ArchiveConfig {
    /// Validates fixed operator inputs. HTTP is allowed only on literal loopback for fixtures.
    /// # Errors
    /// Rejects incomplete credentials, unsafe endpoint components and invalid bucket/region.
    pub fn new(
        endpoint: &str,
        bucket: &str,
        region: &str,
        access: String,
        secret: String,
    ) -> Result<Self, ArchiveObjectError> {
        let endpoint = Url::parse(endpoint).map_err(|_| ArchiveObjectError::Configuration)?;
        let local = matches!(endpoint.host_str(), Some("127.0.0.1" | "[::1]"));
        if !(endpoint.scheme() == "https" || endpoint.scheme() == "http" && local)
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || endpoint.path() != "/"
            || !(3..=63).contains(&bucket.len())
            || !bucket
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || bucket.starts_with('-')
            || bucket.ends_with('-')
            || region.is_empty()
            || region.len() > 64
            || !region
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || access.is_empty()
            || secret.is_empty()
            || access.len() > 4096
            || secret.len() > 4096
            || access.contains(['\r', '\n', '\0'])
            || secret.contains(['\r', '\n', '\0'])
        {
            return Err(ArchiveObjectError::Configuration);
        }
        Ok(Self {
            endpoint,
            bucket: bucket.into(),
            region: region.into(),
            access,
            secret,
        })
    }
}
/// Whole-object identity and exact expected length, suitable for later manifests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveObjectRef {
    tenant: TenantId,
    digest: [u8; 32],
    bytes: u64,
    key: String,
}
impl ArchiveObjectRef {
    /// Validates a bounded object descriptor; it is not authorization or proof of existence.
    /// # Errors
    /// Rejects empty or oversized object lengths.
    pub fn new(tenant: TenantId, digest: [u8; 32], bytes: u64) -> Result<Self, ArchiveObjectError> {
        if bytes == 0 || bytes > MAX_FILE_BYTES as u64 {
            return Err(ArchiveObjectError::Limit);
        }
        let hex = digest.iter().fold(String::with_capacity(64), |mut s, b| {
            use std::fmt::Write as _;
            write!(s, "{b:02x}").expect("String formatting");
            s
        });
        let tenant_key = tenant
            .as_str()
            .as_bytes()
            .iter()
            .fold(String::new(), |mut s, b| {
                use std::fmt::Write as _;
                write!(s, "{b:02x}").expect("String formatting");
                s
            });
        let key = format!("fleetiq/archive/v1/{tenant_key}/{hex}.parquet");
        Ok(Self {
            tenant,
            digest,
            bytes,
            key,
        })
    }
    /// Stable derived key; never accepted as an arbitrary write destination.
    pub fn key(&self) -> &str {
        &self.key
    }
    /// Expected complete-object SHA-256 (not the codec's logical digest).
    pub const fn sha256(&self) -> [u8; 32] {
        self.digest
    }
    /// Expected complete object bytes.
    pub const fn byte_length(&self) -> u64 {
        self.bytes
    }
}
/// Closed, redacted outcomes; no endpoints, credentials or response bodies are exposed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveObjectError {
    /// Invalid operator configuration.
    Configuration,
    /// Object/request exceeded the bounded profile.
    Limit,
    /// Tenant scope differs from the descriptor.
    Scope,
    /// Bytes are not a supported exact segment.
    Segment,
    /// An immutable destination already contains different evidence.
    Conflict,
    /// The exact object is absent.
    Missing,
    /// Stored length/digest/tenant evidence failed verification.
    Integrity,
    /// Network, timeout or backend prerequisite failure.
    Unavailable,
}
impl fmt::Display for ArchiveObjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("archive object operation failed")
    }
}
impl std::error::Error for ArchiveObjectError {}
fn classify(e: &object_store::Error) -> ArchiveObjectError {
    match e {
        object_store::Error::NotFound { .. } => ArchiveObjectError::Missing,
        _ => ArchiveObjectError::Unavailable,
    }
}
/// Create-only immutable segment operations. No delete, overwrite, list or multipart API.
pub struct ImmutableArchiveObjects {
    store: AmazonS3,
}
impl fmt::Debug for ImmutableArchiveObjects {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImmutableArchiveObjects([REDACTED])")
    }
}
impl ImmutableArchiveObjects {
    /// Builds a fixed S3 client without network calls or environment credential discovery.
    /// # Errors
    /// Rejects invalid transport/backend configuration.
    pub fn connect(config: S3ArchiveConfig) -> Result<Self, ArchiveObjectError> {
        let transport = transport::Transport::new(config.endpoint.clone())?;
        let store = AmazonS3Builder::new()
            .with_endpoint(config.endpoint.as_str())
            .with_bucket_name(config.bucket)
            .with_region(config.region)
            .with_access_key_id(config.access)
            .with_secret_access_key(config.secret)
            .with_allow_http(config.endpoint.scheme() == "http")
            .with_virtual_hosted_style_request(false)
            .with_conditional_put(S3ConditionalPut::ETagMatch)
            .with_http_connector(transport)
            .with_retry(RetryConfig {
                max_retries: 0,
                retry_timeout: Duration::from_secs(5),
                ..Default::default()
            })
            .build()
            .map_err(|_| ArchiveObjectError::Configuration)?;
        Ok(Self { store })
    }
    /// Conditionally creates a complete segment and verifies registry-addressable bytes.
    /// # Errors
    /// Rejects invalid segments, divergent existing objects and infrastructure failures.
    pub async fn put(
        &self,
        tenant: &TenantId,
        bytes: &[u8],
    ) -> Result<ArchiveObjectRef, ArchiveObjectError> {
        if bytes.len() > MAX_FILE_BYTES || bytes.is_empty() {
            return Err(ArchiveObjectError::Limit);
        }
        decode(tenant, bytes).map_err(|_| ArchiveObjectError::Segment)?;
        let reference = ArchiveObjectRef::new(
            tenant.clone(),
            Sha256::digest(bytes).into(),
            bytes.len() as u64,
        )?;
        tokio::time::timeout(Duration::from_secs(5), async {
            let result = self
                .store
                .put_opts(
                    &Path::from(reference.key()),
                    Bytes::copy_from_slice(bytes).into(),
                    PutMode::Create.into(),
                )
                .await;
            let existed = match result {
                Ok(_) => false,
                Err(
                    object_store::Error::AlreadyExists { .. }
                    | object_store::Error::Precondition { .. },
                ) => true,
                Err(e) => return Err(classify(&e)),
            };
            self.read(tenant, &reference).await.map_err(|e| {
                if existed
                    && matches!(
                        e,
                        ArchiveObjectError::Integrity | ArchiveObjectError::Missing
                    )
                {
                    ArchiveObjectError::Conflict
                } else {
                    e
                }
            })?;
            Ok(reference)
        })
        .await
        .map_err(|_| ArchiveObjectError::Unavailable)?
    }
    /// Reads exact whole-object bytes, validating length, digest and encoded tenant.
    /// # Errors
    /// Distinguishes absent, corrupt, scope-mismatched and unavailable objects.
    pub async fn get(
        &self,
        tenant: &TenantId,
        reference: &ArchiveObjectRef,
    ) -> Result<Vec<u8>, ArchiveObjectError> {
        tokio::time::timeout(Duration::from_secs(5), self.read(tenant, reference))
            .await
            .map_err(|_| ArchiveObjectError::Unavailable)?
    }
    async fn read(
        &self,
        tenant: &TenantId,
        reference: &ArchiveObjectRef,
    ) -> Result<Vec<u8>, ArchiveObjectError> {
        if &reference.tenant != tenant {
            return Err(ArchiveObjectError::Scope);
        }
        let path = Path::from(reference.key());
        let metadata = self.store.head(&path).await.map_err(|e| classify(&e))?;
        if metadata.size != reference.bytes {
            return Err(ArchiveObjectError::Integrity);
        }
        let result = self
            .store
            .get_opts(
                &path,
                GetOptions {
                    if_match: metadata.e_tag,
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| classify(&e))?;
        if result.meta.size != reference.bytes {
            return Err(ArchiveObjectError::Integrity);
        }
        let bytes = result.bytes().await.map_err(|e| classify(&e))?;
        if bytes.len() as u64 != reference.bytes
            || <[u8; 32]>::from(Sha256::digest(&bytes)) != reference.digest
        {
            return Err(ArchiveObjectError::Integrity);
        }
        decode(tenant, &bytes).map_err(|_| ArchiveObjectError::Integrity)?;
        Ok(bytes.to_vec())
    }
}

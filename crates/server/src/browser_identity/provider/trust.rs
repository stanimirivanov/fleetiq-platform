//! Additive OIDC trust; the protocol client and all its requests share this client.

use std::{fs::File, io::Read};

use super::{BrowserIdentityConfig, OidcError, PROVIDER_REQUEST_TIMEOUT, reqwest};

const MAX_CA_BUNDLE_BYTES: u64 = 256 * 1024;
const MAX_CA_CERTIFICATES: usize = 32;
const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
const END: &str = "-----END CERTIFICATE-----";

pub(super) fn http_client(
    configuration: &BrowserIdentityConfig,
) -> Result<reqwest::Client, OidcError> {
    let mut builder = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(PROVIDER_REQUEST_TIMEOUT)
        .connect_timeout(PROVIDER_REQUEST_TIMEOUT);
    if let Some(path) = configuration.additional_ca_bundle() {
        // Read at most the bound plus one even if a file changes while opening.
        // Never report its path, contents or parser diagnostics.
        let file = File::open(path).map_err(|_| OidcError::Operational)?;
        if !file
            .metadata()
            .map_err(|_| OidcError::Operational)?
            .is_file()
        {
            return Err(OidcError::Operational);
        }
        let mut bytes = Vec::new();
        file.take(MAX_CA_BUNDLE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| OidcError::Operational)?;
        if bytes.len() as u64 > MAX_CA_BUNDLE_BYTES {
            return Err(OidcError::Operational);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| OidcError::Operational)?;
        let mut remaining = text.trim();
        let mut count = 0;
        while !remaining.is_empty() {
            // Refuse mixed private-key/junk/partial blocks; the maintained TLS
            // library owns PEM/DER parsing and certificate validation.
            if !remaining.starts_with(BEGIN) || count >= MAX_CA_CERTIFICATES {
                return Err(OidcError::Operational);
            }
            let end = remaining.find(END).ok_or(OidcError::Operational)? + END.len();
            let certificates = reqwest::Certificate::from_pem_bundle(&remaining.as_bytes()[..end])
                .map_err(|_| OidcError::Operational)?;
            if certificates.len() != 1 {
                return Err(OidcError::Operational);
            }
            let certificate = certificates
                .into_iter()
                .next()
                .ok_or(OidcError::Operational)?;
            builder = builder.add_root_certificate(certificate);
            remaining = remaining[end..].trim();
            count += 1;
        }
        if count == 0 {
            return Err(OidcError::Operational);
        }
    }
    builder.build().map_err(|_| OidcError::Operational)
}

use std::{error::Error, fmt};

use fleetiq_domain::IdentityAuthorityId;
use url::Url;

/// Trusted, deployment-selected browser origin and `OpenID Connect` provider.
///
/// Both endpoints require HTTPS. Provider credentials remain on the server;
/// request headers never select the issuer, client, or callback destination.
#[derive(Clone)]
pub struct BrowserIdentityConfig {
    public_origin: String,
    issuer: String,
    client_id: String,
    client_secret: String,
    identity_authority: IdentityAuthorityId,
}

impl BrowserIdentityConfig {
    /// Validates a deployment's browser identity settings.
    ///
    /// # Errors
    ///
    /// Returns [`BrowserIdentityConfigError`] for non-HTTPS or ambiguous URLs,
    /// invalid identifiers, or empty or excessively long client credentials.
    pub fn from_values(
        public_origin: &str,
        issuer: &str,
        client_id: &str,
        client_secret: &str,
        identity_authority: &str,
    ) -> Result<Self, BrowserIdentityConfigError> {
        let origin = trusted_https_url(public_origin)
            .ok_or(BrowserIdentityConfigError::InvalidPublicOrigin)?;
        if origin.path() != "/" {
            return Err(BrowserIdentityConfigError::InvalidPublicOrigin);
        }
        trusted_https_url(issuer).ok_or(BrowserIdentityConfigError::InvalidIssuer)?;
        if !valid_credential(client_id) || !valid_credential(client_secret) {
            return Err(BrowserIdentityConfigError::InvalidClientCredentials);
        }
        let identity_authority = IdentityAuthorityId::new(identity_authority)
            .map_err(|_| BrowserIdentityConfigError::InvalidIdentityAuthority)?;
        Ok(Self {
            public_origin: origin.origin().ascii_serialization(),
            // Issuer comparison is exact in OIDC; preserve its configured path
            // and trailing slash rather than changing the provider's identity.
            issuer: issuer.to_owned(),
            client_id: client_id.to_owned(),
            client_secret: client_secret.to_owned(),
            identity_authority,
        })
    }

    /// Returns the single trusted public origin, without a trailing slash.
    pub fn public_origin(&self) -> &str {
        &self.public_origin
    }

    /// Returns the deployment-selected issuer URL.
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// Returns the registered authority used to resolve provider subjects.
    pub fn identity_authority(&self) -> &IdentityAuthorityId {
        &self.identity_authority
    }

    /// Returns the fixed same-origin authorization callback URL.
    pub fn callback_url(&self) -> String {
        format!("{}/api/v1/auth/callback", self.public_origin)
    }

    pub(crate) fn client_id(&self) -> &str {
        &self.client_id
    }

    pub(crate) fn client_secret(&self) -> &str {
        &self.client_secret
    }
}

impl fmt::Debug for BrowserIdentityConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BrowserIdentityConfig")
            .field("public_origin", &self.public_origin)
            .field("issuer", &self.issuer)
            .field("identity_authority", &self.identity_authority)
            .field("client_id", &"[redacted]")
            .field("client_secret", &"[redacted]")
            .finish()
    }
}

/// Invalid browser identity configuration without credential-bearing details.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserIdentityConfigError {
    /// The public URL is not an unambiguous HTTPS origin.
    InvalidPublicOrigin,
    /// The issuer URL is not an unambiguous HTTPS endpoint.
    InvalidIssuer,
    /// A client credential is empty, too long, or contains control characters.
    InvalidClientCredentials,
    /// The registered identity authority does not satisfy domain constraints.
    InvalidIdentityAuthority,
}

impl fmt::Display for BrowserIdentityConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPublicOrigin => "browser public origin must be an HTTPS origin",
            Self::InvalidIssuer => {
                "OIDC issuer must be an HTTPS URL without credentials, query, or fragment"
            }
            Self::InvalidClientCredentials => "OIDC client credentials are invalid",
            Self::InvalidIdentityAuthority => "OIDC identity authority is invalid",
        })
    }
}

impl Error for BrowserIdentityConfigError {}

fn trusted_https_url(value: &str) -> Option<Url> {
    if !value.starts_with("https://")
        || value.chars().any(|character| {
            character.is_whitespace() || character.is_control() || character == '\\'
        })
        || value.strip_prefix("https://").is_some_and(|rest| {
            rest.split(['/', '?', '#'])
                .next()
                .is_some_and(|authority| authority.contains('@'))
        })
    {
        return None;
    }
    let parsed = Url::parse(value).ok()?;
    (parsed.scheme() == "https"
        && parsed.host_str().is_some()
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none())
    .then_some(parsed)
}

fn valid_credential(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests;

use std::{convert::Infallible, future::ready};

use fleetiq_access::ExternalIdentityAuthenticator;
use fleetiq_domain::ExternalIdentity;
use subtle::ConstantTimeEq;

use crate::config::BearerIdentityConfig;

/// Validates the configured opaque HTTP credential without trusting request claims.
///
/// Only the server configuration assigns an external identity. An invalid token
/// never reaches the actor resolver. The fixed-width comparison avoids a
/// content-dependent early exit; length and syntax are checked at the HTTP
/// boundary before this adapter is called.
#[derive(Clone)]
pub(crate) struct ConfiguredBearerAuthenticator {
    config: BearerIdentityConfig,
}

impl ConfiguredBearerAuthenticator {
    pub(crate) const fn new(config: BearerIdentityConfig) -> Self {
        Self { config }
    }
}

impl ExternalIdentityAuthenticator for ConfiguredBearerAuthenticator {
    type Credential = String;
    type Error = Infallible;

    fn authenticate<'a>(
        &'a self,
        credential: &'a Self::Credential,
    ) -> impl Future<Output = Result<Option<ExternalIdentity>, Self::Error>> + Send + 'a {
        let valid = <&[u8; 64]>::try_from(credential.as_bytes())
            .is_ok_and(|bytes| bool::from(bytes.ct_eq(self.config.token())));
        ready(Ok(valid.then(|| self.config.external_identity().clone())))
    }
}

#[cfg(test)]
mod tests {
    use fleetiq_access::ExternalIdentityAuthenticator;

    use super::*;
    use crate::ServerConfig;

    #[tokio::test]
    async fn accepts_only_the_configured_credential() {
        let token = "a".repeat(64);
        let config = ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
            .expect("valid server configuration")
            .with_bearer_identity(&token, "workload", "reader")
            .expect("valid bearer configuration");
        let authenticator = ConfiguredBearerAuthenticator::new(
            config
                .bearer_identity()
                .expect("configured credential")
                .clone(),
        );

        assert!(authenticator.authenticate(&token).await.unwrap().is_some());
        assert!(
            authenticator
                .authenticate(&"b".repeat(64))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            authenticator
                .authenticate(&"a".repeat(63))
                .await
                .unwrap()
                .is_none()
        );
    }
}

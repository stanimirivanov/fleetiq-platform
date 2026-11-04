//! Opt-in Alertmanager transport and explicit durable delivery process.
mod payload;
#[cfg(test)]
mod tests;
use fleetiq_analytics::{AlertDeliveryFailure, AlertNotificationLease};
use fleetiq_postgres::PostgresAlertDelivery;
use sha2::{Digest, Sha256};
use std::{error::Error, time::Duration};
use url::Url;

/// One operator-controlled API endpoint. Debug output excludes configuration.
#[derive(Clone)]
pub struct AlertmanagerConfig {
    endpoint: Url,
    destination: [u8; 32],
}
impl std::fmt::Debug for AlertmanagerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AlertmanagerConfig([REDACTED])")
    }
}
impl AlertmanagerConfig {
    /// Validates a full fixed Alertmanager v2 alerts endpoint. Tenants never choose it.
    /// # Errors
    /// Rejects non-HTTP(S), userinfo, query, fragment or a different API path.
    pub fn new(endpoint: &str) -> Result<Self, AlertmanagerConfigError> {
        let endpoint = Url::parse(endpoint).map_err(|_| AlertmanagerConfigError)?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || endpoint.path() != "/api/v2/alerts"
        {
            return Err(AlertmanagerConfigError);
        }
        let destination = Sha256::digest(endpoint.as_str().as_bytes()).into();
        Ok(Self {
            endpoint,
            destination,
        })
    }
}
/// Bounded invalid endpoint configuration; no URL or credential is exposed.
#[derive(Debug)]
pub struct AlertmanagerConfigError;
impl std::fmt::Display for AlertmanagerConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid Alertmanager endpoint configuration")
    }
}
impl Error for AlertmanagerConfigError {}
struct AlertmanagerClient {
    config: AlertmanagerConfig,
    client: reqwest::Client,
}
impl AlertmanagerClient {
    fn new(config: AlertmanagerConfig) -> Result<Self, AlertmanagerConfigError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| AlertmanagerConfigError)?;
        Ok(Self { config, client })
    }
    async fn send(&self, lease: &AlertNotificationLease) -> Result<(), AlertDeliveryFailure> {
        if lease.destination != self.config.destination {
            return Err(AlertDeliveryFailure::InvalidPayload);
        }
        let body = payload::encode(lease)?;
        let mut response = self
            .client
            .post(self.config.endpoint.clone())
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|_| AlertDeliveryFailure::Transport)?;
        // Do not follow Location or accept an HTML redirect as delivery. No error
        // body, URL, label, credential or remote exception enters logs/storage.
        if response.status() != reqwest::StatusCode::OK {
            return Err(AlertDeliveryFailure::Rejected);
        }
        let mut bytes = 0usize;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| AlertDeliveryFailure::Transport)?
        {
            bytes = bytes
                .checked_add(chunk.len())
                .ok_or(AlertDeliveryFailure::ResponseLimit)?;
            if bytes > 8192 {
                return Err(AlertDeliveryFailure::ResponseLimit);
            }
        }
        Ok(())
    }
}
/// Runs delivery only in the explicit notification-worker process. Requires
/// `DATABASE_URL` and `FLEETIQ_ALERTMANAGER_URL`; never performs DDL or starts HTTP.
/// # Errors
/// Returns redacted startup failures. Failed/cancelled turns retain durable retry.
pub async fn run_notification_worker() -> Result<(), Box<dyn Error>> {
    let config = AlertmanagerConfig::new(
        &std::env::var("FLEETIQ_ALERTMANAGER_URL").map_err(|_| "Alertmanager endpoint required")?,
    )?;
    let client = AlertmanagerClient::new(config)?;
    let database = std::env::var("DATABASE_URL").map_err(|_| "database configuration required")?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(3))
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET statement_timeout = '5s'")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&database)
        .await
        .map_err(|_| "notification database unavailable")?;
    let result=async {
        crate::SchemaMode::Check.prepare(&pool).await.map_err(|_|"notification schema incompatible")?;
        let work=PostgresAlertDelivery::new(pool.clone());
        let shutdown=crate::runtime::shutdown_signal();tokio::pin!(shutdown);
        loop {
            let turn=async {
                work.seed(&client.config.destination).await?;
                if let Some(lease)=work.claim(&client.config.destination).await? {
                    let outcome=client.send(&lease).await;
                    if !work.finish(&lease,outcome).await? {eprintln!("notification lease expired or superseded");}
                }
                Ok::<(),Box<dyn Error>>(())
            };
            tokio::select!{()=&mut shutdown=>break,result=tokio::time::timeout(Duration::from_secs(15),turn)=>{
                if !matches!(result,Ok(Ok(()))){eprintln!("notification turn failed; durable retry retained");}
            }}
            tokio::select!{()=&mut shutdown=>break,()=tokio::time::sleep(Duration::from_millis(250))=>{}}
        }
        Ok::<(),Box<dyn Error>>(())
    }.await;
    pool.close().await;
    result
}

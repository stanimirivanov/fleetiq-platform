//! `FleetIQ` backend process entry point.

use std::error::Error;

use fleetiq_server::ServerConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    fleetiq_server::run(ServerConfig::from_env()?).await?;
    Ok(())
}

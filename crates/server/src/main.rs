//! `FleetIQ` backend process entry point.

use std::{env, error::Error};

use fleetiq_identity::PlatformBootstrapOutcome;
use fleetiq_server::{
    PlatformBootstrapConfig, ProcessCommand, ServerConfig, bootstrap_platform_operator,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    match ProcessCommand::parse(env::args_os().skip(1))? {
        ProcessCommand::Serve => fleetiq_server::run(ServerConfig::from_env()?).await?,
        ProcessCommand::BootstrapPlatformOperator => {
            let outcome = bootstrap_platform_operator(PlatformBootstrapConfig::from_env()?).await?;
            match outcome {
                PlatformBootstrapOutcome::Bootstrapped => {
                    println!("platform operator bootstrapped");
                }
                PlatformBootstrapOutcome::AlreadyBootstrapped => {
                    println!("platform operator was already bootstrapped");
                }
            }
        }
        ProcessCommand::Help => println!("{}", ProcessCommand::usage()),
    }
    Ok(())
}

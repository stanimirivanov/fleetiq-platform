//! `FleetIQ` backend process entry point.

mod health_probe;

use std::{env, error::Error, process::ExitCode};

use fleetiq_identity::PlatformBootstrapOutcome;
use fleetiq_server::{
    MigrationConfig, PlatformBootstrapConfig, ProcessCommand, SchemaMode, ServerConfig,
    bootstrap_platform_operator, migrate_database, migration_inventory,
};

#[tokio::main]
async fn main() -> ExitCode {
    match execute().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Render the bounded boundary message, never an error/source tree.
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn execute() -> Result<(), Box<dyn Error>> {
    match ProcessCommand::parse(env::args_os().skip(1))? {
        ProcessCommand::Serve => fleetiq_server::run(ServerConfig::from_env()?).await?,
        ProcessCommand::ServeChecked => {
            fleetiq_server::run(ServerConfig::from_env()?.with_schema_mode(SchemaMode::Check))
                .await?;
        }
        ProcessCommand::Migrate => {
            migrate_database(MigrationConfig::from_env()?).await?;
            println!("embedded migrations applied and verified");
        }
        ProcessCommand::MigrationInventory => println!("{}", migration_inventory()?),
        ProcessCommand::Healthcheck => health_probe::check().await?,
        command @ (ProcessCommand::BootstrapPlatformOperator
        | ProcessCommand::BootstrapPlatformOperatorChecked) => {
            let mode = if command == ProcessCommand::BootstrapPlatformOperatorChecked {
                SchemaMode::Check
            } else {
                SchemaMode::Migrate
            };
            let outcome = bootstrap_platform_operator(
                PlatformBootstrapConfig::from_env()?.with_schema_mode(mode),
            )
            .await?;
            match outcome {
                PlatformBootstrapOutcome::Bootstrapped => {
                    println!("platform operator bootstrapped");
                }
                PlatformBootstrapOutcome::AlreadyBootstrapped => {
                    println!("platform operator was already bootstrapped");
                }
            }
        }
        ProcessCommand::ReplayWorker => fleetiq_server::run_replay_worker().await?,
        ProcessCommand::Help => println!("{}", ProcessCommand::usage()),
    }
    Ok(())
}

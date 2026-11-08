//! `FleetIQ` backend process entry point.

mod health_probe;

use std::{env, error::Error, process::ExitCode};

use fleetiq_history::retention_work::RetentionTick;
use fleetiq_identity::PlatformBootstrapOutcome;
use fleetiq_server::{
    MigrationConfig, PlatformBootstrapConfig, ProcessCommand, SchemaMode, ServerConfig,
    ServerError, bootstrap_platform_operator, migrate_database, migration_inventory,
};

#[tokio::main]
async fn main() -> ExitCode {
    match execute().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Render the bounded boundary message, never an error/source tree.
            eprintln!("{error}");
            // A Tokio runtime drop can wait for outstanding blocking work.
            // The deadline is a process boundary, including runtime teardown.
            if matches!(
                error.downcast_ref::<ServerError>(),
                Some(ServerError::ShutdownDeadline)
            ) {
                std::process::exit(1);
            }
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
        ProcessCommand::IngestTcp => {
            fleetiq_server::run_tcp_ingestion(fleetiq_server::TcpIngestionConfig::from_env()?)
                .await?;
        }
        ProcessCommand::ReplayWorker => fleetiq_server::run_replay_worker().await?,
        ProcessCommand::SemanticWorker => fleetiq_server::run_semantic_worker().await?,
        ProcessCommand::ProjectionWorker => fleetiq_server::run_projection_worker().await?,
        ProcessCommand::RebuildCurrent => fleetiq_server::rebuild_current_state().await?,
        ProcessCommand::RegisterCalculator => {
            fleetiq_server::register_calculator_definition().await?;
            println!("calculator declaration registered or already present");
        }
        ProcessCommand::NotificationWorker => fleetiq_server::run_notification_worker().await?,
        ProcessCommand::AnalyticsWorker => fleetiq_server::run_analytics_worker().await?,
        ProcessCommand::ArchiveWorker => fleetiq_server::run_archive_worker().await?,
        ProcessCommand::ReclaimOnce => match fleetiq_server::run_reclaim_once().await? {
            RetentionTick::Disabled => println!("disabled"),
            RetentionTick::Protected => println!("protected"),
            RetentionTick::Denied => println!("denied"),
            RetentionTick::Reclaimed(count) => println!("reclaimed {count}"),
            RetentionTick::AlreadyReclaimed(count) => println!("already-reclaimed {count}"),
        },
        ProcessCommand::SummarizeOnce => {
            let receipt = fleetiq_server::run_summarize_once().await?;
            println!(
                "summary series={} revision={}",
                receipt.series, receipt.revision
            );
        }
        ProcessCommand::CalculateOnce => {
            fleetiq_server::calculate_once().await?;
            println!("calculator evaluation persisted");
        }
        ProcessCommand::Help => println!("{}", ProcessCommand::usage()),
    }
    Ok(())
}

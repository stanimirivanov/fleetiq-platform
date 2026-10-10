//! Repository-owned development commands for `FleetIQ`.

mod architecture;
mod cli;
mod migrations;
mod repository;
mod secrets;

use std::{env, process::ExitCode};

fn main() -> ExitCode {
    match cli::parse(env::args().skip(1)) {
        Ok(cli::Command::Architecture) => run_architecture_check(),
        Ok(cli::Command::Repository) => run_repository_check(),
        Ok(cli::Command::Migrations { base }) => run_migration_check(&base),
        Ok(cli::Command::Secrets { base }) => run_secrets_check(base.as_deref()),
        Ok(cli::Command::SecretFixtures) => run_secret_fixtures(),
        Ok(cli::Command::Help) => {
            print_usage();
            ExitCode::SUCCESS
        }
        Err(()) => {
            eprintln!("invalid xtask command or arguments");
            print_usage();
            ExitCode::FAILURE
        }
    }
}

fn run_secrets_check(base: Option<&str>) -> ExitCode {
    let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    match secrets::check(root, base) {
        Ok(report) => {
            println!(
                "secret policy passed for {} reviewable files; incoming history checked: {}",
                report.file_count(),
                report.history_checked()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("secret policy failed:\n{error}");
            ExitCode::FAILURE
        }
    }
}

fn run_secret_fixtures() -> ExitCode {
    let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    match secrets::verify_fixtures(root) {
        Ok(count) => {
            println!("pinned secret detector passed {count} bypass regression scenarios");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("secret detector fixtures failed:\n{error}");
            ExitCode::FAILURE
        }
    }
}

fn run_migration_check(base: &str) -> ExitCode {
    match migrations::check(base) {
        Ok(report) => {
            println!(
                "migration policy passed against {}: {} immutable migrations and {} additions",
                report.base_commit(),
                report.immutable_count(),
                report.added_count()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("migration policy failed:\n{error}");
            ExitCode::FAILURE
        }
    }
}

fn run_repository_check() -> ExitCode {
    match repository::check() {
        Ok(report) => {
            println!(
                "repository policy passed for {} Markdown documents",
                report.document_count()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("repository policy failed:\n{error}");
            ExitCode::FAILURE
        }
    }
}

fn run_architecture_check() -> ExitCode {
    match architecture::check() {
        Ok(report) => {
            println!(
                concat!(
                    "architecture policy passed for {} workspace packages, {} internal dependencies, ",
                    "{} external dependencies, and {} inherited lint policies"
                ),
                report.package_count(),
                report.dependency_count(),
                report.external_dependency_count(),
                report.lint_inheritance_count()
            );
            for exception in report.temporary_exceptions() {
                println!("temporary exception: {exception}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("architecture policy failed:\n{error}");
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    eprintln!(
        "FleetIQ repository tasks\n\n\
         Usage:\n\
           cargo xtask architecture           Verify dependency ownership and inherited lints\n\
           cargo xtask repository             Verify documentation and repository policy\n\
           cargo xtask migrations --base REV  Protect published migrations\n\
           cargo xtask secrets [--base REV]   Scan files, index, and optional incoming history\n\
           cargo xtask secrets --fixtures     Verify the pinned detector against bypasses"
    );
}

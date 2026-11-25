//! Exercise the native application commands using only a clean committed source tree.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::migrations::git::command as git;

pub(super) fn run(root: &Path) -> Result<(), String> {
    super::nextest::check(root)?;
    checked(
        git(root).args(["diff", "--quiet", "HEAD", "--"]),
        "commit tracked changes before proving application isolation",
    )?;
    let scratch = Scratch::create()?;
    let archive = scratch.0.join("source.tar");
    let application = scratch.0.join("application");
    fs::create_dir(&application).map_err(|_| "could not create the application directory")?;
    checked(
        git(root)
            .arg("archive")
            .args(["--format=tar", "--output"])
            .arg(&archive)
            .arg("HEAD"),
        "archive the exact committed source",
    )?;
    checked(
        Command::new("tar")
            .arg("-xf")
            .arg(&archive)
            .arg("-C")
            .arg(&application),
        "unpack the committed source",
    )?;
    fs::remove_file(archive).map_err(|_| "could not remove the source archive")?;
    fs::remove_dir_all(application.join("harness"))
        .map_err(|_| "could not remove harness/ from the isolated tree")?;

    checked(
        cargo(&application).env("SQLX_OFFLINE", "true").args([
            "build",
            "--workspace",
            "--all-features",
            "--locked",
        ]),
        "build the locked application workspace without harness/",
    )?;
    let mut tests = cargo(&application);
    tests.args([
        "nextest",
        "run",
        "--workspace",
        "--all-features",
        "--locked",
        "--profile",
        "ci",
    ]);
    super::nextest::isolate(&mut tests);
    checked(
        &mut tests,
        "run the full application test suite without harness/",
    )?;
    checked(
        cargo(&application).args(["test", "--workspace", "--all-features", "--locked", "--doc"]),
        "run application doctests without harness/",
    )?;
    println!("application isolation passed: locked build, full Nextest suite, and doctests");
    Ok(())
}

fn cargo(application: &Path) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(application)
        .env("CARGO_TARGET_DIR", application.join("target"))
        .env("CARGO_INCREMENTAL", "0")
        .env_remove("CARGO_BUILD_BUILD_DIR")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER");
    command
}

fn checked(command: &mut Command, operation: &str) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|error| format!("could not {operation}: {}", error.kind()))?;
    if !status.success() {
        return Err(format!("failed to {operation} ({status})"));
    }
    Ok(())
}

struct Scratch(PathBuf);

impl Scratch {
    fn create() -> Result<Self, String> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "could not name the isolation directory")?;
        let parent = std::env::temp_dir()
            .canonicalize()
            .map_err(|_| "could not resolve the temporary directory")?;
        let directory = parent.join(format!(
            "fleetiq-app-isolation-{}-{}",
            std::process::id(),
            stamp.as_nanos()
        ));
        fs::create_dir(&directory).map_err(|_| "could not create a fresh isolation directory")?;
        Ok(Self(directory))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if fs::remove_dir_all(&self.0).is_err() {
            eprintln!("could not clean up the application isolation directory");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_commands_have_a_private_source_and_target_directory() {
        let scratch = Scratch::create().unwrap();
        let command = cargo(&scratch.0);
        let target = scratch.0.join("target");
        assert_eq!(command.get_current_dir(), Some(scratch.0.as_path()));
        assert!(
            command.get_envs().any(
                |(name, value)| name == "CARGO_TARGET_DIR" && value == Some(target.as_os_str())
            )
        );
        assert!(
            command
                .get_envs()
                .any(|(name, value)| name == "RUSTC_WORKSPACE_WRAPPER" && value.is_none())
        );
    }
}

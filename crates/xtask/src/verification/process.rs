//! Process and prerequisite checks; environment values never enter evidence.

mod toolchain;

use std::{collections::BTreeMap, env, path::Path, process::Command};

use serde_json::{Value, json};

use super::{
    catalogue::{Check, Group, Prerequisite},
    ci, evidence,
    runner::{Executor, Outcome, ResultEntry},
};

pub(super) struct System<'a> {
    root: &'a Path,
    context: Result<Context, String>,
    base: Result<Option<String>, String>,
    tools: BTreeMap<&'static str, Result<String, String>>,
    wiring: Result<(), String>,
}

struct Context {
    head: String,
    dirty: bool,
}

impl<'a> System<'a> {
    pub(super) fn new(root: &'a Path, base: Option<&str>, checks: &[&Check]) -> Self {
        let wiring = ci::check(root);
        let context = context(root);
        let base = base.map_or(Ok(None), |base| {
            crate::migrations::git::resolve_base(root, base)
                .map(Some)
                .map_err(|_| "trusted base unavailable or not an ancestor; fetch history and select the actual event base".into())
        });
        let mut tools = BTreeMap::new();
        for check in checks {
            for tool in check.tools {
                tools
                    .entry(tool.name())
                    .or_insert_with(|| toolchain::probe(root, *tool));
            }
        }
        Self {
            root,
            context,
            base,
            tools,
            wiring,
        }
    }

    /// HEAD and clean/dirty changes invalidate the group. These observations
    /// deliberately do not pretend to hash already-dirty file contents.
    pub(super) fn finish(&self) -> Result<(), String> {
        let start = self.context.as_ref().map_err(Clone::clone)?;
        let end = context(self.root)?;
        if start.head != end.head || start.dirty != end.dirty {
            return Err("checkout revision or clean/dirty state changed during verification; rerun on stable source".into());
        }
        ci::check(self.root)
    }

    pub(super) fn report(&self, group: Group, entries: &[ResultEntry], passed: bool) -> Value {
        let end = context(self.root);
        json!({
            "schema_version": 1,
            "group": group.name(),
            "complete_pass": passed,
            "source": self.context.as_ref().ok().map(|context| json!({ "commit": context.head, "dirty": context.dirty })),
            "source_at_end": end.as_ref().ok().map(|context| json!({ "commit": context.head, "dirty": context.dirty })),
            "trusted_base": self.base.as_ref().ok().and_then(Option::as_ref),
            "tool_observations": self.tools.iter().map(|(name, observation)| json!({
                "tool": name,
                "version": observation.as_ref().ok(),
                "blocker": observation.as_ref().err(),
            })).collect::<Vec<_>>(),
            "checks": entries.iter().map(|entry| {
                let mut value = evidence::inventory_entry(entry.check);
                value["result"] = evidence::outcome(&entry.outcome);
                value["elapsed_ms"] = json!(entry.elapsed_ms);
                value["execution_adapter"] = json!(if is_xtask(entry.check) { "current-xtask" } else { "external-tool" });
                value["executed_args"] = match entry.outcome {
                    Outcome::NotRun { .. } => Value::Null,
                    _ => json!(execution_arguments(entry.check, self.base.as_ref().ok().and_then(Option::as_deref))),
                };
                value
            }).collect::<Vec<_>>(),
        })
    }
}

impl Executor for System<'_> {
    fn prerequisite(&mut self, check: &Check) -> Result<(), String> {
        self.wiring.as_ref().map_err(Clone::clone)?;
        self.context.as_ref().map_err(Clone::clone)?;
        for tool in check.tools {
            self.tools[tool.name()].as_ref().map_err(Clone::clone)?;
        }
        for prerequisite in check.prerequisites {
            match prerequisite {
                Prerequisite::TrustedBase => {
                    if self.base.as_ref().map_err(Clone::clone)?.is_none() {
                        return Err("explicit trusted base required; use --base with the current published/event base".into());
                    }
                }
                Prerequisite::Database => database_prerequisite(
                    env::var_os("DATABASE_URL").is_some_and(|value| !value.is_empty()),
                    env::var_os("SQLX_OFFLINE").is_some(),
                )?,
                Prerequisite::NextestConfiguration => super::nextest::check(self.root)?,
                Prerequisite::OpenapiDependencies => {
                    if !self.root.join("node_modules/.bin/redocly").is_file() {
                        return Err(
                            "locked OpenAPI tools unavailable; run pnpm install --frozen-lockfile"
                                .into(),
                        );
                    }
                }
            }
        }
        Ok(())
    }

    fn execute(&mut self, check: &Check) -> Outcome {
        let args = execution_arguments(check, self.base.as_ref().ok().and_then(Option::as_deref));
        if is_xtask(check) {
            // Rebuilding the running executable fails on Windows. Launch the
            // exact compiled task instead of recursively invoking cargo run.
            match env::current_exe() {
                Ok(executable) => {
                    execute_command(Command::new(executable).current_dir(self.root).args(&args))
                }
                Err(_) => Outcome::NotRun {
                    reason: "compiled xtask executable is unavailable".into(),
                },
            }
        } else if check.id == "tests" {
            let mut command = Command::new(check.program);
            command.current_dir(self.root).args(&args);
            super::nextest::isolate(&mut command);
            execute_command(&mut command)
        } else {
            run_program(self.root, check.program, &args)
        }
    }
}

fn arguments(check: &Check, base: Option<&str>) -> Vec<String> {
    check
        .args
        .iter()
        .map(|argument| {
            if *argument == "@base" {
                base.unwrap_or_default().to_owned()
            } else {
                (*argument).to_owned()
            }
        })
        .collect()
}

fn is_xtask(check: &Check) -> bool {
    check.program == "cargo" && check.args.first() == Some(&"xtask")
}

fn execution_arguments(check: &Check, base: Option<&str>) -> Vec<String> {
    let mut args = arguments(check, base);
    if is_xtask(check) {
        args.remove(0);
    }
    args
}
fn database_prerequisite(url_present: bool, offline_present: bool) -> Result<(), String> {
    if !url_present {
        return Err("DATABASE_URL is unset/empty; configure fully migrated PostgreSQL 18.6; credentials are never recorded".into());
    }
    if offline_present {
        return Err("SQLX_OFFLINE is set; unset it for online PostgreSQL acceptance checks".into());
    }
    Ok(())
}

fn context(root: &Path) -> Result<Context, String> {
    let head = crate::migrations::git::command(root)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .map_err(|_| "Git checkout metadata is unavailable")?;
    let head_text = std::str::from_utf8(&head.stdout).unwrap_or_default().trim();
    if !head.status.success() || !valid_hash(head_text) {
        return Err("Git checkout metadata is unavailable; use a real checkout with HEAD".into());
    }
    let status = crate::migrations::git::command(root)
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=normal"])
        .output()
        .map_err(|_| "Git worktree state is unavailable")?;
    if !status.status.success() {
        return Err("Git worktree state is unavailable".into());
    }
    Ok(Context {
        head: head_text.to_owned(),
        dirty: !status.stdout.is_empty(),
    })
}

fn valid_hash(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn program_name(program: &str) -> &str {
    if cfg!(windows) && program == "pnpm" {
        "pnpm.cmd"
    } else {
        program
    }
}

/// Standard tool output stays on the console; it is not copied into artifacts.
/// Only the fixed pnpm arguments use its Windows command shim. Dynamic Git
/// references are resolved before execution and passed directly to cargo.
fn run_program(root: &Path, program: &str, args: &[String]) -> Outcome {
    execute_command(
        Command::new(program_name(program))
            .current_dir(root)
            .args(args),
    )
}

fn execute_command(command: &mut Command) -> Outcome {
    isolate_cargo_context(command);
    match command
        .env("RUSTFLAGS", "-D warnings")
        .env("RUSTDOCFLAGS", "-D warnings")
        .status()
    {
        Ok(status) if status.success() => Outcome::Passed,
        Ok(status) => Outcome::Failed {
            exit_code: status.code(),
        },
        Err(_) => Outcome::NotRun {
            reason: "could not start the inventoried command; check pinned tool installation"
                .into(),
        },
    }
}

/// Cargo run injects its own package metadata. cargo-machete uses that metadata
/// to distinguish Cargo subcommand arguments from cargo-run arguments; leaking
/// it makes even a valid version probe fail. Remove only invocation metadata,
/// preserving the selected toolchain, jobserver, database, and caller options.
fn isolate_cargo_context(command: &mut Command) {
    for (name, _) in env::vars_os() {
        let normalized = name.to_string_lossy().to_ascii_uppercase();
        if normalized.starts_with("CARGO_PKG_")
            || matches!(
                normalized.as_str(),
                "CARGO_MANIFEST_DIR"
                    | "CARGO_MANIFEST_PATH"
                    | "CARGO_PRIMARY_PACKAGE"
                    | "CARGO_BIN_NAME"
            )
        {
            command.env_remove(name);
        }
    }
    // Also isolate explicitly configured fixture/launcher metadata rather than
    // requiring global environment mutation in parallel tests.
    for name in [
        "CARGO_PKG_NAME",
        "CARGO_MANIFEST_DIR",
        "CARGO_MANIFEST_PATH",
        "CARGO_PRIMARY_PACKAGE",
        "CARGO_BIN_NAME",
    ] {
        command.env_remove(name);
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_url_presence_is_not_a_database_test_and_offline_cannot_replace_online_checks() {
        assert!(database_prerequisite(false, false).is_err());
        assert!(database_prerequisite(true, true).is_err());
        assert!(database_prerequisite(true, false).is_ok());
    }

    #[test]
    fn base_argument_uses_resolved_commit_without_shell_interpolation() {
        let check = super::super::catalogue::CHECKS
            .iter()
            .find(|check| check.id == "migrations")
            .unwrap();
        let args = arguments(check, Some("1234567890123456789012345678901234567890"));
        assert_eq!(
            args.last().unwrap(),
            "1234567890123456789012345678901234567890"
        );
        assert!(
            !args
                .iter()
                .any(|arg| arg == "origin/main" || arg == "@base")
        );
    }

    #[test]
    fn cargo_run_package_metadata_cannot_change_external_subcommand_interpretation() {
        let mut command = Command::new("cargo");
        command
            .env("CARGO_PKG_NAME", "fleetiq-xtask")
            .env("CARGO_MANIFEST_DIR", "launcher")
            .env("CARGO_BIN_NAME", "fleetiq-xtask")
            .env("DATABASE_URL", "fixture-connection")
            .env("RUSTUP_TOOLCHAIN", "1.99.1");
        isolate_cargo_context(&mut command);
        let overrides: BTreeMap<_, _> = command.get_envs().collect();
        assert_eq!(overrides[std::ffi::OsStr::new("CARGO_PKG_NAME")], None);
        assert_eq!(overrides[std::ffi::OsStr::new("CARGO_MANIFEST_DIR")], None);
        assert_eq!(overrides[std::ffi::OsStr::new("CARGO_BIN_NAME")], None);
        assert_eq!(
            overrides[std::ffi::OsStr::new("DATABASE_URL")],
            Some(std::ffi::OsStr::new("fixture-connection"))
        );
        assert_eq!(
            overrides[std::ffi::OsStr::new("RUSTUP_TOOLCHAIN")],
            Some(std::ffi::OsStr::new("1.99.1"))
        );
    }
    #[test]
    fn compiled_task_invocations_cannot_trigger_a_nested_cargo_rebuild() {
        let task = super::super::catalogue::CHECKS
            .iter()
            .find(|check| check.id == "migrations")
            .unwrap();
        assert!(is_xtask(task));
        assert_eq!(
            execution_arguments(task, Some("resolved")),
            ["migrations", "--base", "resolved"]
        );
        let external = super::super::catalogue::CHECKS
            .iter()
            .find(|check| check.id == "sensor-tests")
            .unwrap();
        assert!(!is_xtask(external));
        assert_eq!(
            execution_arguments(external, None),
            arguments(external, None)
        );
    }
    #[test]
    fn unavailable_user_supplied_base_and_environment_values_never_enter_evidence() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let system = System::new(root, Some("do-not-record-this-invalid-reference"), &[]);
        let report = system.report(Group::Architecture, &[], false);
        let rendered = serde_json::to_string(&report).unwrap();
        assert!(!rendered.contains("do-not-record-this-invalid-reference"));
        assert!(report["trusted_base"].is_null());
        assert!(report.get("environment").is_none());
        assert!(report.get("stdout").is_none());
        assert!(report.get("stderr").is_none());
        assert_eq!(report["source"]["commit"].as_str().unwrap().len(), 40);
    }
    #[test]
    fn real_process_success_failure_and_missing_executable_are_distinct() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        assert_eq!(
            run_program(root, "rustc", &["--version".into()]),
            Outcome::Passed
        );
        assert!(matches!(
            run_program(root, "rustc", &["--nonexistent-harness-fixture".into()]),
            Outcome::Failed { .. }
        ));
        assert!(matches!(
            run_program(root, "fleetiq-no-such-fixture-executable", &[]),
            Outcome::NotRun { .. }
        ));
    }
}

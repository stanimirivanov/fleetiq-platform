//! Runs default detector rules and exposes only safe report metadata.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::Value;

use super::{
    SecretsError, VERSION,
    configuration::{self, ExactException},
    failure,
    snapshot::sanitize_environment,
};

const FINDINGS_EXIT: i32 = 23;

pub(super) struct Detector(PathBuf);

impl Detector {
    pub(super) fn discover(root: &Path) -> Result<Self, SecretsError> {
        let local = root.join("target/tools/gitleaks").join(if cfg!(windows) {
            "gitleaks.exe"
        } else {
            "gitleaks"
        });
        let executable = if local.is_file() {
            local
        } else {
            PathBuf::from("gitleaks")
        };
        let mut command = Command::new(&executable);
        command.arg("version");
        sanitize_environment(&mut command);
        let output = command.output().map_err(|_| {
            failure("Gitleaks is unavailable; run node scripts/install-gitleaks.mjs")
        })?;
        let version = std::str::from_utf8(&output.stdout)
            .unwrap_or_default()
            .trim();
        if !output.status.success() || version != VERSION {
            return Err(failure(format!(
                "Gitleaks must be the pinned {VERSION}; run node scripts/install-gitleaks.mjs"
            )));
        }
        Ok(Self(executable))
    }

    pub(super) fn scan_directory(
        &self,
        directory: &Path,
        policy: &Path,
        temporary: &Path,
    ) -> Result<(), SecretsError> {
        self.scan("dir", directory, &[], policy, temporary)
    }

    pub(super) fn scan_history(
        &self,
        repository: &Path,
        range: &str,
        policy: &Path,
        temporary: &Path,
    ) -> Result<(), SecretsError> {
        let options = format!("--text --no-ext-diff --no-textconv --no-renames {range}");
        self.scan(
            "git",
            repository,
            &["--log-opts", &options],
            policy,
            temporary,
        )
    }

    fn scan(
        &self,
        mode: &str,
        source: &Path,
        arguments: &[&str],
        policy: &Path,
        temporary: &Path,
    ) -> Result<(), SecretsError> {
        let policy_source = fs::read_to_string(policy)
            .map_err(|_| failure("could not read the exact exception policy"))?;
        let exceptions = configuration::exceptions(&policy_source)?;
        let defaults = temporary.join("detector-defaults.toml");
        let ignore_path = temporary.join("empty-ignore");
        // Never feed exceptions to the pinned binary: its global path handling
        // can broaden an AND exception into a whole-file exclusion.
        fs::write(&defaults, b"[extend]\nuseDefault = true\n")
            .map_err(|_| failure("could not prepare default detector rules"))?;
        fs::write(&ignore_path, b"")
            .map_err(|_| failure("could not isolate detector fingerprint ignores"))?;
        let mut command = Command::new(&self.0);
        command
            .current_dir(source)
            .arg(mode)
            .arg(".")
            .args(arguments)
            .arg("--config")
            .arg(&defaults)
            .arg("--gitleaks-ignore-path")
            .arg(&ignore_path)
            .args([
                "--redact=0",
                "--ignore-gitleaks-allow",
                "--no-banner",
                "--no-color",
                "--report-format",
                "json",
                "--report-path",
                "-",
                "--exit-code",
                "23",
                "--timeout",
                "120",
            ]);
        sanitize_environment(&mut command);
        let output = command
            .output()
            .map_err(|_| failure("could not run the pinned secret detector"))?;
        // An unredacted JSON pipe is needed for exact secret equality. No raw
        // report is written to disk; stdout/stderr are never forwarded, even on
        // failure. Only vetted metadata leaves this module.
        match output.status.code() {
            Some(0 | FINDINGS_EXIT) => {}
            _ => {
                return Err(failure(
                    "secret detector failed; check its pinned installation, policy, and Git history",
                ));
            }
        }
        let (raw_count, findings) = filtered_findings(&output.stdout, source, &exceptions)?;
        if (output.status.code() == Some(FINDINGS_EXIT)) != (raw_count > 0) {
            return Err(failure(
                "secret detector returned contradictory status and report contents",
            ));
        }
        if findings.is_empty() {
            return Ok(());
        }
        Err(failure(format!(
            "secret findings:\n{}",
            findings.join("\n")
        )))
    }
}

#[cfg(test)]
pub(super) fn safe_findings(bytes: &[u8], source: &Path) -> Result<Vec<String>, SecretsError> {
    filtered_findings(bytes, source, &[]).map(|(_, findings)| findings)
}

/// Ignore Match, Line, Description, identity, and URLs. Secret is compared only
/// against an exact owned exception for its exact path, never rendered/logged.
pub(super) fn filtered_findings(
    bytes: &[u8],
    source: &Path,
    exceptions: &[ExactException],
) -> Result<(usize, Vec<String>), SecretsError> {
    let report: Value = serde_json::from_slice(bytes)
        .map_err(|_| failure("secret detector report was not valid JSON"))?;
    let findings = report
        .as_array()
        .ok_or_else(|| failure("secret detector report was not an array"))?;
    let mut secrets = Vec::new();
    for finding in findings {
        let secret = finding
            .get("Secret")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| failure("secret detector report lacked private value evidence"))?;
        if !secret.is_empty() {
            secrets.push(secret);
        }
    }
    secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    secrets.dedup();
    let mut safe = Vec::new();
    for finding in findings {
        let file = finding
            .get("File")
            .and_then(Value::as_str)
            .ok_or_else(|| failure("secret detector report lacked a file location"))?;
        let path = Path::new(file);
        let relative = path.strip_prefix(source).unwrap_or(path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(failure(
                "secret detector reported a path outside its selected inputs",
            ));
        }
        let line = finding
            .get("StartLine")
            .and_then(Value::as_u64)
            .filter(|line| *line > 0)
            .ok_or_else(|| failure("secret detector report lacked a positive line number"))?;
        let rule = finding
            .get("RuleID")
            .and_then(Value::as_str)
            .filter(|rule| {
                !rule.is_empty()
                    && rule.len() <= 120
                    && rule
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
            .ok_or_else(|| {
                failure("secret detector report contained an invalid rule identifier")
            })?;
        let path = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if exceptions
            .iter()
            .any(|exception| exception.matches_path(&path))
        {
            let secret = finding
                .get("Secret")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    failure("exact exception matching requires private detector value evidence")
                })?;
            if exceptions
                .iter()
                .any(|exception| exception.matches(&path, secret))
            {
                continue;
            }
        }
        let mut public_path = path;
        let mut public_rule = rule.to_owned();
        for secret in &secrets {
            public_path = public_path.replace(secret, "REDACTED");
            public_path =
                public_path.replace(&secret.replace(std::path::MAIN_SEPARATOR, "/"), "REDACTED");
            public_rule = public_rule.replace(secret, "REDACTED");
        }
        safe.push(format!("- {public_path:?}:{line} [{public_rule}]; remove the credential and follow private incident handling"));
    }
    safe.sort();
    safe.dedup();
    Ok((findings.len(), safe))
}

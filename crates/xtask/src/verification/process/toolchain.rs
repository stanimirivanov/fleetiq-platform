//! Discovers pinned tools and validates safe version metadata.
//!
//! Compatibility with individual tool version formats stays separate from
//! source context, database prerequisites, command execution, and reports.

use std::{path::Path, process::Command};

use super::{isolate_cargo_context, program_name};
use crate::verification::catalogue::Tool;
pub(super) fn probe(root: &Path, tool: Tool) -> Result<String, String> {
    let (program, args): (&str, &[&str]) = match tool {
        Tool::Rust => ("rustc", &["--version"]),
        Tool::Git => ("git", &["--version"]),
        Tool::Gitleaks => {
            let name = if cfg!(windows) {
                "gitleaks.exe"
            } else {
                "gitleaks"
            };
            let local = root.join("target/tools/gitleaks").join(name);
            let program = if local.is_file() {
                local.into_os_string()
            } else {
                "gitleaks".into()
            };
            return probe_command(Command::new(program).arg("version"), tool);
        }
        Tool::Deny => ("cargo", &["deny", "--version"]),
        Tool::Machete => ("cargo", &["machete", "--version"]),
        Tool::Sqlx => ("cargo", &["sqlx", "--version"]),
        Tool::Nextest => ("cargo", &["nextest", "--version"]),
        Tool::Node => ("node", &["--version"]),
        Tool::Pnpm => (program_name("pnpm"), &["--version"]),
    };
    probe_command(Command::new(program).current_dir(root).args(args), tool)
}

fn probe_command(command: &mut Command, tool: Tool) -> Result<String, String> {
    isolate_cargo_context(command);
    let output = command.output().map_err(|_| {
        format!(
            "{} unavailable; install pinned {}",
            tool.name(),
            tool.expected()
        )
    })?;
    let version = std::str::from_utf8(&output.stdout)
        .ok()
        .and_then(parse_version);
    let version = version.filter(|_| output.status.success()).ok_or_else(|| {
        format!(
            "{} version probe failed; install pinned {}",
            tool.name(),
            tool.expected()
        )
    })?;
    if !version_matches(tool, version) {
        return Err(format!(
            "{} has unsupported version {version}; required {}",
            tool.name(),
            tool.expected()
        ));
    }
    Ok(version.to_owned())
}

/// Only numeric version tokens may enter the metadata report. Arbitrary
/// subprocess diagnostics, credentials, and environment values stay out.
fn parse_version(value: &str) -> Option<&str> {
    value
        .split_whitespace()
        .map(|part| part.strip_prefix('v').unwrap_or(part))
        .find(|part| {
            let part = *part;
            let core = part.split_once(".windows.").map_or(part, |(core, suffix)| {
                if !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit()) {
                    core
                } else {
                    part
                }
            });
            let fields: Vec<_> = core.split('.').collect();
            fields.len() == 3
                && fields.iter().all(|field| {
                    !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit())
                })
        })
}

fn version_matches(tool: Tool, version: &str) -> bool {
    match tool {
        Tool::Git => true,
        Tool::Node => {
            version.split('.').count() == 3 && version.split('.').next() == Some(tool.expected())
        }
        _ => version == tool.expected(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_version_or_untrusted_version_text_cannot_pass_or_enter_report() {
        assert!(version_matches(Tool::Rust, "1.99.1"));
        assert!(version_matches(Tool::Nextest, "0.9.145"));
        assert!(!version_matches(Tool::Nextest, "0.9.144"));
        assert!(!version_matches(Tool::Rust, "1.98.1"));
        assert!(version_matches(Tool::Node, "24.18.0"));
        assert!(!version_matches(Tool::Node, "25.0.0"));
        assert_eq!(parse_version("token=secret not-a-version"), None);
        assert_eq!(parse_version("rustc 1.99.1 (hash date)"), Some("1.99.1"));
        assert_eq!(parse_version("v24.18.0"), Some("24.18.0"));
        assert_eq!(
            parse_version("git version 2.55.0.windows.3"),
            Some("2.55.0.windows.3")
        );
        assert!(!version_matches(Tool::Rust, "1.99.1.windows.3"));
    }
}

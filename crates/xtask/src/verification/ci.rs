//! Exact generated-workflow verification without a second YAML interpreter.
//!
//! The reviewed template owns setup, service, permission, and cleanup semantics.
//! The catalogue supplies runner groups and CLI versions. Comparing the entire
//! rendered document rejects disabled, conditional, or commented-out checks as
//! well as changed commands. Only immutable action dependency refs are supplied
//! by the workflow itself; their repositories and locations stay template-owned.

#[cfg(test)]
mod tests;

mod actions;

use std::{fs, path::Path};

use super::catalogue::{CHECKS, Group, Tool};

const TEMPLATE: &str = ".github/ci.yml.template";
const WORKFLOW: &str = ".github/workflows/ci.yml";

/// Verifies the checked-in workflow matches its reviewed template and inventory.
pub(crate) fn check(root: &Path) -> Result<(), String> {
    let actual = read_regular(root, WORKFLOW)?;
    let expected = render(&read_regular(root, TEMPLATE)?, &actual)?;
    check_document(&actual, &expected)
}

/// Explicitly regenerates the workflow after an intentional policy update.
///
/// Ordinary verification never calls this function or rewrites stale content.
pub(crate) fn write(root: &Path) -> Result<(), String> {
    // Requiring an existing regular destination avoids following a symlink or
    // creating a second workflow when the reviewed CI entry point is missing.
    let actual = read_regular(root, WORKFLOW)?;
    let expected = render(&read_regular(root, TEMPLATE)?, &actual)?;
    if actual == expected {
        return Ok(());
    }
    fs::write(root.join(WORKFLOW), expected)
        .map_err(|error| format!("cannot write {WORKFLOW}: {}", error.kind()))
}

fn read_regular(root: &Path, relative: &str) -> Result<String, String> {
    let mut candidate = root.to_path_buf();
    for component in Path::new(relative).components() {
        candidate.push(component);
        let metadata = fs::symlink_metadata(&candidate)
            .map_err(|error| format!("cannot read {relative}: {}", error.kind()))?;
        if metadata.file_type().is_symlink() || redirected(&metadata) {
            return Err(format!("{relative} must not use a symlink or junction"));
        }
    }
    if !candidate.is_file() {
        return Err(format!("{relative} must be a regular file"));
    }
    fs::read_to_string(candidate)
        .map_err(|error| format!("cannot read {relative} as UTF-8: {}", error.kind()))
}

// Windows directory junctions can redirect a regular-looking policy path.
#[cfg(windows)]
fn redirected(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn redirected(_: &fs::Metadata) -> bool {
    false
}
fn normalize(document: &str) -> Result<String, String> {
    let normalized = document.replace("\r\n", "\n");
    if normalized.contains('\r') {
        return Err(
            "CI policy requires LF or CRLF line endings, without bare carriage returns".into(),
        );
    }
    Ok(normalized)
}

fn render(template: &str, workflow: &str) -> Result<String, String> {
    let mut result = normalize(template)?;
    for group in [
        Group::Architecture,
        Group::SupplyChain,
        Group::Rust,
        Group::Openapi,
    ] {
        let token = format!("@verify:{}@", group.name());
        let mut invocation = format!("cargo xtask verify --group {}", group.name());
        if group == Group::Architecture {
            invocation.push_str(" --base \"$HARNESS_BASE\"");
        }
        let count = if group == Group::Openapi { 2 } else { 1 };
        replace_token(&mut result, &token, &invocation, count)?;
    }
    // Supplemental Windows steps use existing inventory commands rather than
    // maintaining a second command list or pretending to run the full gate.
    for id in ["repository", "architecture", "sensor-tests"] {
        let check = CHECKS
            .iter()
            .find(|check| check.id == id)
            .ok_or_else(|| format!("verification catalogue is missing {id}"))?;
        let count = if id == "sensor-tests" { 1 } else { 2 };
        replace_token(&mut result, &format!("@check:{id}@"), check.command, count)?;
    }
    for (name, tool, count) in [
        ("deny", Tool::Deny, 1),
        ("machete", Tool::Machete, 1),
        ("sqlx", Tool::Sqlx, 1),
        ("pnpm", Tool::Pnpm, 2),
        ("node", Tool::Node, 3),
    ] {
        replace_token(
            &mut result,
            &format!("@tool:{name}@"),
            tool.expected(),
            count,
        )?;
    }
    actions::substitute(&mut result, &normalize(workflow)?)?;
    if result.contains("@verify:")
        || result.contains("@tool:")
        || result.contains("@check:")
        || result.contains("@action:")
    {
        return Err(format!(
            "{TEMPLATE} contains an unknown verification placeholder"
        ));
    }
    Ok(result)
}

fn replace_token(
    document: &mut String,
    token: &str,
    replacement: &str,
    expected_count: usize,
) -> Result<(), String> {
    if document.matches(token).count() != expected_count {
        return Err(format!(
            "{TEMPLATE} requires exactly {expected_count} occurrences of {token}"
        ));
    }
    *document = document.replace(token, replacement);
    Ok(())
}

fn check_document(actual: &str, expected: &str) -> Result<(), String> {
    let actual = normalize(actual)?;
    if actual == expected {
        return Ok(());
    }
    let line = actual
        .lines()
        .zip(expected.lines())
        .position(|(left, right)| left != right)
        .map_or_else(
            || actual.lines().count().min(expected.lines().count()) + 1,
            |index| index + 1,
        );
    Err(format!(
        "{WORKFLOW}:{line} differs from the reviewed template and verification catalogue; \
         review policy changes, then run cargo xtask verify --write-ci; \
         policy: docs/development/verification.md"
    ))
}

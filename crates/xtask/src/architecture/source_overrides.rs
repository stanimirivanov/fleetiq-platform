//! Rejects repository-owned source overrides before Cargo metadata hides them.
//!
//! Direct dependency declarations do not reveal transitive `[patch]` resolution,
//! deprecated `[replace]`, configuration path overrides, or source replacement.
//! This sensor reads only the root manifest and both root Cargo configuration
//! filenames. Ambient parent/user configuration and command-line overrides are
//! outside its scope and belong to controlled build execution.

#[cfg(test)]
mod tests;

use std::{collections::BTreeSet, fs, io::ErrorKind, path::Path};

use toml::Table;

const GUIDE: &str = "docs/architecture/dependency-rules.md";

/// Rejects active override declarations in the repository's Cargo policy files.
///
/// Both configuration filenames are checked even when Cargo gives one precedence.
/// Missing optional configurations are allowed; unreadable or malformed files
/// cannot provide passing evidence. No file contents or source URLs enter errors.
pub(super) fn check(workspace_root: &Path) -> Result<(), Vec<String>> {
    let files = [
        (workspace_root.join("Cargo.toml"), false),
        (workspace_root.join(".cargo/config.toml"), true),
        (workspace_root.join(".cargo/config"), true),
    ];
    let mut violations = BTreeSet::new();
    for (path, configuration) in files {
        match read_document(&path, configuration) {
            Ok(Some(document)) => {
                for detail in document_violations(&document, configuration) {
                    violations.insert(violation(&path, &detail));
                }
            }
            Ok(None) => {}
            Err(detail) => {
                violations.insert(violation(&path, &detail));
            }
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations.into_iter().collect())
    }
}

fn read_document(path: &Path, optional: bool) -> Result<Option<Table>, String> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if optional && error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "could not read Cargo policy file ({})",
                error.kind()
            ));
        }
    };
    // Parser errors may contain keys or source excerpts. Report only the file
    // and failure category so credentials in malformed configuration stay out
    // of diagnostics and CI logs.
    source
        .parse()
        .map(Some)
        .map_err(|_: toml::de::Error| "could not parse Cargo policy file as TOML".to_owned())
}

fn document_violations(document: &Table, configuration: bool) -> Vec<String> {
    let mut violations = Vec::new();
    record_override(
        patch_present(document),
        "repository-owned Cargo [patch] overrides are not approved; remove patch entries and review dependency ownership",
        &mut violations,
    );
    if configuration {
        record_override(
            paths_present(document),
            "repository-owned Cargo paths overrides are not approved; remove paths entries and add reviewed workspace members",
            &mut violations,
        );
        record_override(
            source_replacement_present(document),
            "repository-owned Cargo [source] replace-with overrides are not approved; remove source replacement directives",
            &mut violations,
        );
    } else {
        record_override(
            replace_present(document),
            "repository-owned Cargo [replace] overrides are not approved; remove replacement entries and review dependency ownership",
            &mut violations,
        );
    }
    violations
}

fn record_override(result: Result<bool, String>, detail: &str, violations: &mut Vec<String>) {
    match result {
        Ok(true) => violations.push(detail.to_owned()),
        Ok(false) => {}
        Err(error) => violations.push(error),
    }
}

fn patch_present(document: &Table) -> Result<bool, String> {
    let Some(patch) = table(document, "patch")? else {
        return Ok(false);
    };
    let mut present = false;
    for source in patch.values() {
        let entries = source
            .as_table()
            .ok_or_else(|| "Cargo [patch] source entries must be tables".to_owned())?;
        present |= !entries.is_empty();
    }
    Ok(present)
}

fn replace_present(document: &Table) -> Result<bool, String> {
    Ok(table(document, "replace")?.is_some_and(|entries| !entries.is_empty()))
}

fn paths_present(document: &Table) -> Result<bool, String> {
    let Some(paths) = document.get("paths") else {
        return Ok(false);
    };
    paths
        .as_array()
        .map(|entries| !entries.is_empty())
        .ok_or_else(|| "Cargo paths must be an array".to_owned())
}

fn source_replacement_present(document: &Table) -> Result<bool, String> {
    let Some(sources) = table(document, "source")? else {
        return Ok(false);
    };
    let mut present = false;
    for source in sources.values() {
        let source = source
            .as_table()
            .ok_or_else(|| "Cargo [source] entries must be tables".to_owned())?;
        present |= source.contains_key("replace-with");
    }
    Ok(present)
}

fn table<'a>(document: &'a Table, section: &str) -> Result<Option<&'a Table>, String> {
    document
        .get(section)
        .map(|value| {
            value
                .as_table()
                .ok_or_else(|| format!("Cargo [{section}] must be a table"))
        })
        .transpose()
}

fn violation(path: &Path, detail: &str) -> String {
    format!(
        "Cargo source override policy ({}): {detail}; see {GUIDE}",
        path.display()
    )
}

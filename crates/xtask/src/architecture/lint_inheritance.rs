//! Prevents workspace packages from opting out of the shared Rust lint policy.
//!
//! Cargo metadata selects the manifests, including newly added members. Cargo
//! does not expose their lint declarations, so a TOML parser checks those files
//! rather than searching source text that comments or strings could spoof.

#[cfg(test)]
mod tests;

use std::{fs, path::Path};

use cargo_metadata::Metadata;
use toml::Value;

const GUIDE: &str = "docs/architecture/dependency-rules.md";

/// Requires every Cargo-selected workspace package to inherit the shared lints.
///
/// Returns the checked member count. Read and parse failures are violations,
/// since an unavailable manifest cannot provide passing verification evidence.
pub(super) fn check(metadata: &Metadata) -> Result<usize, Vec<String>> {
    let packages = metadata.workspace_packages();
    let mut violations = Vec::new();
    for package in &packages {
        if let Err(error) =
            check_manifest(package.name.as_ref(), package.manifest_path.as_std_path())
        {
            violations.push(error);
        }
    }
    violations.sort();
    if violations.is_empty() {
        Ok(packages.len())
    } else {
        Err(violations)
    }
}

fn check_manifest(package: &str, path: &Path) -> Result<(), String> {
    let source = fs::read_to_string(path)
        .map_err(|error| violation(package, path, &format!("could not read manifest: {error}")))?;
    validate_manifest(&source).map_err(|detail| violation(package, path, &detail))
}

fn validate_manifest(source: &str) -> Result<(), String> {
    let document: toml::Table = source.parse().map_err(|error: toml::de::Error| {
        // Report the parser's reason without copying manifest source into logs.
        format!("could not parse manifest as TOML: {}", error.message())
    })?;
    let lints = document
        .get("lints")
        .and_then(Value::as_table)
        .ok_or_else(|| {
            "declare [lints] with workspace = true to inherit the shared Rust lint policy"
                .to_owned()
        })?;
    if lints.get("workspace").and_then(Value::as_bool) != Some(true) {
        return Err("[lints].workspace must be the boolean true; package-local lint policies are not approved".to_owned());
    }
    if lints.keys().any(|key| key != "workspace") {
        return Err("remove package-local lint tables and keep only [lints] workspace = true; review shared policy changes at the workspace root".to_owned());
    }
    Ok(())
}

fn violation(package: &str, path: &Path, detail: &str) -> String {
    format!(
        "workspace lint policy for `{package}` ({}): {detail}; see {GUIDE}",
        path.display()
    )
}

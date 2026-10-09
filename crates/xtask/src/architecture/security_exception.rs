//! Keeps the approved verification-only RSA advisory exception exact.
//!
//! The pinned `cargo-deny` checks wrapper names, but its version ranges exclude
//! prereleases. The lockfile supplies the complete graph, including dependencies
//! inactive on the current host, so no added RSA consumer escapes this review.

#[cfg(test)]
mod tests;

use std::{fs, path::Path};

use toml::Value;

const REVIEW: &str = "docs/development/plans/browser-identity-dependency-review.md";
const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";
const OIDC_VERSION: &str = "4.0.1";
const RSA_VERSION: &str = "0.9.10";

/// Checks the complete locked graph independently of target and feature filters.
pub(super) fn check(path: &Path) -> Result<(), String> {
    let source = fs::read_to_string(path)
        .map_err(|error| violation(&format!("could not read {}: {error}", path.display())))?;
    validate_lockfile(&source)
}

fn validate_lockfile(source: &str) -> Result<(), String> {
    let document: toml::Table = source
        .parse()
        .map_err(|error| violation(&format!("could not parse Cargo.lock: {error}")))?;
    if document.get("version").and_then(Value::as_integer) != Some(4) {
        return Err(violation(
            "Cargo.lock must use the reviewed version 4 format",
        ));
    }
    let packages = document
        .get("package")
        .and_then(Value::as_array)
        .ok_or_else(|| violation("Cargo.lock has no package array"))?;
    for package in packages {
        validate_record(package)?;
    }
    let oidc = approved_package(packages, "openidconnect", OIDC_VERSION)?;
    approved_package(packages, "rsa", RSA_VERSION)?;

    for package in packages {
        for dependency in dependencies(package) {
            if dependency.split_ascii_whitespace().next() != Some("rsa") {
                continue;
            }
            let consumer = required_string(package, "name")?;
            if consumer != "openidconnect" {
                return Err(violation(&format!(
                    "unreviewed RSA consumer `{consumer}`; only openidconnect {OIDC_VERSION} is approved"
                )));
            }
            if !approved_rsa_reference(dependency) {
                return Err(violation(&format!(
                    "unreviewed RSA dependency reference `{dependency}`"
                )));
            }
        }
    }
    if !dependencies(oidc).any(approved_rsa_reference) {
        return Err(violation(
            "the reviewed openidconnect -> rsa edge is missing; review or remove the stale exception",
        ));
    }
    Ok(())
}

fn validate_record(package: &Value) -> Result<(), String> {
    required_string(package, "name")?;
    required_string(package, "version")?;
    if let Some(source) = package.get("source")
        && source.as_str().is_none_or(str::is_empty)
    {
        return Err(violation(
            "Cargo.lock package source must be a nonempty string",
        ));
    }
    if let Some(dependencies) = package.get("dependencies") {
        let dependencies = dependencies
            .as_array()
            .ok_or_else(|| violation("Cargo.lock dependencies must be an array"))?;
        if dependencies
            .iter()
            .any(|dependency| dependency.as_str().is_none_or(str::is_empty))
        {
            return Err(violation(
                "Cargo.lock dependency references must be nonempty strings",
            ));
        }
    }
    Ok(())
}

fn approved_package<'a>(
    packages: &'a [Value],
    name: &str,
    version: &str,
) -> Result<&'a Value, String> {
    let mut matches = packages
        .iter()
        .filter(|package| package.get("name").and_then(Value::as_str) == Some(name));
    let package = matches
        .next()
        .ok_or_else(|| violation(&format!("reviewed package `{name}` is missing")))?;
    if matches.next().is_some() {
        return Err(violation(&format!(
            "multiple `{name}` packages are outside the approved exception"
        )));
    }
    let actual_version = required_string(package, "version")?;
    if actual_version != version {
        return Err(violation(&format!(
            "unreviewed `{name}` version `{actual_version}`; expected exactly `{version}`"
        )));
    }
    if package.get("source").and_then(Value::as_str) != Some(REGISTRY) {
        return Err(violation(&format!(
            "reviewed package `{name}` must come from crates.io"
        )));
    }
    Ok(package)
}

fn required_string<'a>(package: &'a Value, field: &str) -> Result<&'a str, String> {
    package
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            violation(&format!(
                "Cargo.lock package `{field}` must be a nonempty string"
            ))
        })
}

fn dependencies(package: &Value) -> impl Iterator<Item = &str> {
    // Every record has already been checked, so filtering cannot hide bad types.
    package
        .get("dependencies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

fn approved_rsa_reference(reference: &str) -> bool {
    // Cargo omits qualifiers when package names or versions are unambiguous.
    reference == "rsa"
        || reference == "rsa 0.9.10"
        || reference == "rsa 0.9.10 (registry+https://github.com/rust-lang/crates.io-index)"
}

fn violation(detail: &str) -> String {
    format!("RUSTSEC-2023-0071 exception: {detail}; see {REVIEW}")
}

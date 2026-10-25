//! Enforces the entire reviewed public EC verification consumer closure.
//!
//! Checking only crypto-bigint's immediate parent would miss a new signing or
//! key-agreement consumer elsewhere in the elliptic-curve stack. Every incoming
//! edge to the protected nodes is therefore exact, including the local root.

#[cfg(test)]
mod tests;

use toml::Value;

use super::{REGISTRY, dependencies, validate_record};

const REVIEW: &str = "docs/development/plans/oidc-crypto-bigint-advisory-review.md";
const PACKAGES: &[(&str, &str)] = &[
    ("crypto-bigint", "0.5.5"),
    ("elliptic-curve", "0.13.8"),
    ("ecdsa", "0.16.9"),
    ("primeorder", "0.13.6"),
    ("p256", "0.13.2"),
    ("p384", "0.13.1"),
    ("openidconnect", "4.0.1"),
    ("fleetiq-server", "0.1.0"),
];
const EDGES: &[(&str, &str)] = &[
    ("fleetiq-server", "openidconnect"),
    ("openidconnect", "p256"),
    ("openidconnect", "p384"),
    ("p256", "ecdsa"),
    ("p256", "elliptic-curve"),
    ("p256", "primeorder"),
    ("p384", "ecdsa"),
    ("p384", "elliptic-curve"),
    ("p384", "primeorder"),
    ("ecdsa", "elliptic-curve"),
    ("primeorder", "elliptic-curve"),
    ("elliptic-curve", "crypto-bigint"),
];

/// Rejects graph expansion even for packages inactive on this build target.
pub(super) fn validate_lockfile(source: &str) -> Result<(), String> {
    let document: toml::Table = source
        .parse()
        .map_err(|_| violation("Cargo.lock is not valid TOML"))?;
    if document.get("version").and_then(Value::as_integer) != Some(4) {
        return Err(violation("Cargo.lock must use version 4"));
    }
    let packages = document
        .get("package")
        .and_then(Value::as_array)
        .ok_or_else(|| violation("Cargo.lock has no package array"))?;
    for package in packages {
        validate_record(package).map_err(|_| violation("malformed package record"))?;
    }
    for &(name, version) in PACKAGES {
        validate_package(packages, name, version)?;
    }

    let mut observed = std::collections::BTreeSet::new();
    for package in packages {
        // All records have been validated before examining dependency strings.
        let consumer = package["name"].as_str().expect("validated name");
        for reference in dependencies(package) {
            let Some(&(target, version)) = PACKAGES
                .iter()
                .find(|&&(name, _)| reference.split_ascii_whitespace().next() == Some(name))
            else {
                continue;
            };
            let edge = (consumer, target);
            if !EDGES.contains(&edge) {
                return Err(violation(&format!(
                    "unreviewed consumer edge {consumer} -> {target}"
                )));
            }
            if !approved_reference(reference, target, version) {
                return Err(violation(&format!(
                    "unreviewed dependency reference for {target}"
                )));
            }
            if !observed.insert(edge) {
                return Err(violation("duplicate protected dependency edge"));
            }
        }
    }
    for edge in EDGES {
        if !observed.contains(edge) {
            return Err(violation(&format!(
                "required edge {} -> {} is missing; review or remove the stale exception",
                edge.0, edge.1
            )));
        }
    }
    Ok(())
}

fn validate_package(packages: &[Value], name: &str, version: &str) -> Result<(), String> {
    let mut matches = packages
        .iter()
        .filter(|package| package["name"].as_str() == Some(name));
    let package = matches
        .next()
        .ok_or_else(|| violation(&format!("reviewed package {name} is missing")))?;
    if matches.next().is_some() {
        return Err(violation(&format!("duplicate reviewed package {name}")));
    }
    if package["version"].as_str() != Some(version) {
        return Err(violation(&format!(
            "unreviewed version of {name}; expected exactly {version}"
        )));
    }
    // The composition root must remain local; all reviewed upstream nodes must
    // retain their registry identity. A path/git replacement cannot inherit trust.
    let source = package.get("source");
    let valid_source = if name == "fleetiq-server" {
        source.is_none()
    } else {
        source.and_then(Value::as_str) == Some(REGISTRY)
    };
    if !valid_source {
        return Err(violation(&format!("unreviewed source of {name}")));
    }
    Ok(())
}

fn approved_reference(reference: &str, name: &str, version: &str) -> bool {
    reference == name
        || reference == format!("{name} {version}")
        || (name != "fleetiq-server" && reference == format!("{name} {version} ({REGISTRY})"))
}

fn violation(detail: &str) -> String {
    format!("RUSTSEC-2026-0387 exception: {detail}; see {REVIEW}")
}

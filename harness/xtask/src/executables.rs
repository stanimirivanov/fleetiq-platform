//! Checks exact executable registrations in the trusted Git tree and candidate snapshots.

mod git;
mod registry;
mod surfaces;
#[cfg(test)]
mod tests;

use std::{collections::BTreeSet, path::Path};

use registry::Inventory;
use surfaces::Surface;

const PATH: &str = "harness/executables.json";
const GUIDE: &str = "harness/README.md#executable-registration";

pub(crate) fn check(root: &Path, base: &str) -> Result<(), String> {
    let base =
        crate::migrations::git::resolve_base(root, base).map_err(|error| error.to_string())?;
    let trusted = git::Snapshot::tree(root, &base)?;
    let published = trusted
        .read(PATH)?
        .map(|bytes| Inventory::parse(&bytes))
        .transpose()?;
    let grandfathered = match published {
        Some(inventory) => inventory.baseline,
        None => trusted.surfaces()?,
    };
    let head = git::Snapshot::tree(root, "HEAD")?;
    let index = git::Snapshot::index(root)?;
    let local = git::Snapshot::worktree(root)?;
    // The first rollout can run before its registry is staged. Any tracked
    // candidate snapshot must use its own registry once the trusted base has one.
    for (snapshot, label) in [
        (&head, "committed"),
        (&index, "staged"),
        (&local, "working tree"),
    ] {
        let observed = snapshot.surfaces()?;
        let inventory = snapshot
            .read(PATH)?
            .map(|bytes| Inventory::parse(&bytes))
            .transpose()?;
        let Some(inventory) = inventory else {
            if trusted.contains(PATH)
                || !observed.is_subset(&grandfathered)
                || label == "working tree"
            {
                return Err(format!(
                    "EXEC_REGISTRY FAIL missing executable registry ({label})\nFix: restore {PATH}\nRead: {GUIDE}"
                ));
            }
            continue;
        };
        enforce(&grandfathered, &inventory, &observed, label)?;
        if !inventory.sensors.is_empty() {
            let manifest = snapshot.read("harness/manifest.yaml")?.unwrap_or_default();
            registry::validate_sensors(&inventory.sensors, &manifest)?;
        }
    }
    println!("executable registration passed for committed, staged, and working tree files");
    Ok(())
}

fn enforce(
    grandfathered: &BTreeSet<Surface>,
    inventory: &Inventory,
    observed: &BTreeSet<Surface>,
    label: &str,
) -> Result<(), String> {
    let mut failures = Vec::new();
    for entry in inventory.baseline.difference(grandfathered) {
        failures.push(format!(
            "EXEC_REGISTRY FAIL new surface in the legacy baseline ({label}, {entry})\nWhy: the baseline preserves reviewed legacy entries\nFix: register the new surface in registrations"
        ));
    }
    for entry in inventory.registrations.difference(observed) {
        failures.push(format!(
            "EXEC_REGISTRY FAIL registration has no executable surface ({label}, {entry})\nWhy: new registrations must accompany a concrete executable addition\nFix: remove the unused entry from {PATH}"
        ));
    }
    for entry in observed {
        if !inventory.baseline.contains(entry) && !inventory.registrations.contains(entry) {
            failures.push(format!(
                "EXEC_REGISTRY FAIL unregistered executable surface ({label}, {entry})\nWhy: executable additions need an explicit owner and purpose\nFix: reuse an existing registered entrypoint or register this surface in {PATH}"
            ));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        failures.push(format!("Read: {GUIDE}"));
        Err(failures.join("\n"))
    }
}

//! One repository-owned acceptance inventory, execution, and metadata evidence.
//!
//! Commands remain ordinary tools rather than an alternate test framework.
//! Missing prerequisites fail the group without claiming a test ran.

pub(crate) mod catalogue;
pub(crate) mod ci;
mod evidence;
mod nextest;
mod process;
mod runner;

pub(crate) use catalogue::Group;

use std::path::Path;

/// Prints the executable inventory without claiming verification occurred.
pub(crate) fn list() -> Result<(), String> {
    let entries: Vec<_> = catalogue::CHECKS
        .iter()
        .map(evidence::inventory_entry)
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&entries).map_err(|_| "cannot format check inventory")?
    );
    Ok(())
}

/// Runs every selected check and writes a fresh, ignored metadata-only report.
///
/// Returns false if any check failed or could not run. The base is resolved to
/// an ancestor commit before it can appear in command arguments or evidence.
pub(crate) fn run(root: &Path, group: Group, base: Option<&str>) -> Result<bool, String> {
    runner::run(root, group, base)
}

//! Fresh ignored JSON artifacts contain only selected metadata.

use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    catalogue::{Check, Group},
    runner::Outcome,
};

pub(super) fn inventory_entry(check: &Check) -> Value {
    json!({
        "id": check.id,
        "group": check.group.name(),
        "command": check.command,
        "program": check.program,
        "args": check.args,
        "acceptance": check.acceptance,
        "outcome": check.outcome,
        "prerequisites": check.prerequisites.iter().map(|item| json!({ "name": item.name(), "requirement": item.description() })).collect::<Vec<_>>(),
        "tools": check.tools.iter().map(|tool| json!({ "name": tool.name(), "required": tool.expected() })).collect::<Vec<_>>(),
    })
}

pub(super) fn outcome(outcome: &Outcome) -> Value {
    match outcome {
        Outcome::Passed => json!({ "status": "passed", "exit_code": 0 }),
        Outcome::Failed { exit_code } => json!({ "status": "failed", "exit_code": exit_code }),
        Outcome::NotRun { reason } => json!({ "status": "not-run", "reason": reason }),
    }
}

/// Creates a distinct report per invocation. No arbitrary output path, process
/// output, URL, or environment dump can enter this artifact. Reject redirects
/// through existing symlinks/junctions before creating the owned directory.
pub(super) fn write(root: &Path, group: Group, report: &Value) -> Result<String, String> {
    let relative = Path::new("target/verification");
    for part in [root.join("target"), root.join(relative)] {
        match fs::symlink_metadata(&part) {
            Ok(metadata) if metadata.file_type().is_symlink() || redirected(&metadata) => {
                return Err(
                    "verification evidence directory must not be a symlink or junction".into(),
                );
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err("verification evidence parent is not a directory".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("cannot inspect verification evidence directory".into()),
        }
    }
    fs::create_dir_all(root.join(relative))
        .map_err(|_| "cannot create verification evidence directory")?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "cannot timestamp evidence")?;
    let name = format!(
        "{}-{}-{}.json",
        group.name(),
        std::process::id(),
        timestamp.as_nanos()
    );
    let path = root.join(relative).join(&name);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "cannot create fresh verification evidence")?;
    let mut bytes =
        serde_json::to_vec_pretty(report).map_err(|_| "cannot serialize verification evidence")?;
    bytes.push(b'\n');
    file.write_all(&bytes)
        .map_err(|_| "cannot write verification evidence")?;
    Ok(format!("target/verification/{name}"))
}

#[cfg(windows)]
fn redirected(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn redirected(_: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "fleetiq-verification-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn repeated_invocations_never_reuse_stale_reports() {
        let root = Fixture::new();
        let report = json!({ "schema_version": 1, "complete_pass": false, "checks": [{ "result": outcome(&Outcome::NotRun { reason: "missing tool".into() }) }] });
        let first = write(&root.0, Group::Rust, &report).unwrap();
        let second = write(&root.0, Group::Rust, &report).unwrap();
        assert_ne!(first, second);
        let saved: Value = serde_json::from_slice(&fs::read(root.0.join(first)).unwrap()).unwrap();
        assert_eq!(saved, report);
        assert_eq!(saved["checks"][0]["result"]["status"], "not-run");
    }

    #[cfg(unix)]
    #[test]
    fn redirected_evidence_parent_cannot_write_outside_owned_checkout() {
        let root = Fixture::new();
        let outside = Fixture::new();
        std::os::unix::fs::symlink(&outside.0, root.0.join("target")).unwrap();
        assert!(write(&root.0, Group::Rust, &json!({})).is_err());
        assert!(fs::read_dir(&outside.0).unwrap().next().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn redirected_evidence_parent_cannot_write_outside_owned_checkout() {
        let root = Fixture::new();
        let outside = Fixture::new();
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(root.0.join("target"))
            .arg(&outside.0)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(
            status.success(),
            "junction fixture must be created rather than skipped"
        );
        assert!(write(&root.0, Group::Rust, &json!({})).is_err());
        assert!(fs::read_dir(&outside.0).unwrap().next().is_none());
        fs::remove_dir(root.0.join("target")).unwrap();
    }
    #[test]
    fn non_directory_evidence_parent_fails_without_overwriting_it() {
        let root = Fixture::new();
        fs::write(root.0.join("target"), "existing").unwrap();
        assert!(write(&root.0, Group::Rust, &json!({})).is_err());
        assert_eq!(
            fs::read_to_string(root.0.join("target")).unwrap(),
            "existing"
        );
    }
}

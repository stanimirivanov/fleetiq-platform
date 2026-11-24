use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use serde_json::{Value, json};

use super::{PATH, check};
use crate::migrations::git::command;

static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let fixture = Self(std::env::temp_dir().join(format!(
            "fleetiq-executables-{}-{}",
            std::process::id(),
            FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
        )));
        fs::create_dir(&fixture.0).unwrap();
        fixture.git(&["init", "-q"]);
        fixture.git(&["config", "user.name", "Fixture"]);
        fixture.git(&["config", "user.email", "fixture@example.invalid"]);
        fixture.write("legacy.sh", "echo legacy\n");
        fixture.write("data", "plain file\n");
        fixture.write("launcher", "plain file\n");
        fixture.git(&["add", "."]);
        fixture.git(&["commit", "-qm", "trusted fixtures"]);
        fixture.git(&["branch", "trusted"]);
        fixture.inventory(&[]);
        fixture
    }

    fn write(&self, path: &str, content: &str) {
        let file = self.0.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
    }

    fn git(&self, args: &[&str]) {
        let result = command(&self.0).args(args).output().unwrap();
        assert!(result.status.success(), "fixture Git failed: {args:?}");
    }

    fn inventory(&self, registrations: &[Value]) {
        self.write(
            PATH,
            &json!({
                "schema_version": 1,
                "baseline": [script("legacy.sh")],
                "registrations": registrations
            })
            .to_string(),
        );
    }

    fn check(&self) -> Result<(), String> {
        check(Path::new(&self.0), "trusted")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn script(path: &str) -> Value {
    json!({
        "path": path, "kind": "script", "target": "",
        "class": "operation", "owner": "fleetiq-platform",
        "purpose": "Run a disposable fixture", "disposition": "retain"
    })
}

#[test]
fn legacy_script_edits_pass_and_new_paths_require_registration() {
    let fixture = Fixture::new();
    fixture.write("legacy.sh", "echo maintainable\n");
    fixture.check().unwrap();
    fs::rename(fixture.0.join("legacy.sh"), fixture.0.join("renamed.sh")).unwrap();
    assert!(fixture.check().unwrap_err().contains("renamed.sh"));
    fixture.inventory(&[script("renamed.sh")]);
    fixture.check().unwrap();
}

#[test]
fn new_surface_cannot_be_grandfathered_by_editing_the_baseline() {
    let fixture = Fixture::new();
    fixture.write("new.sh", "echo new\n");
    fixture.write(
        PATH,
        &json!({
            "schema_version": 1,
            "baseline": [script("legacy.sh"), script("new.sh")],
            "registrations": []
        })
        .to_string(),
    );
    assert!(
        fixture
            .check()
            .unwrap_err()
            .contains("new surface in the legacy baseline")
    );
}

#[test]
fn detects_a_new_shebang_and_inspects_staged_bytes_independently() {
    let fixture = Fixture::new();
    fixture.git(&["add", PATH]);
    fixture.git(&["commit", "-qm", "install registry"]);
    fixture.write("launcher", "#!/bin/sh\necho fixture\n");
    assert!(fixture.check().unwrap_err().contains("launcher"));
    fixture.git(&["add", "launcher"]);
    fixture.write("launcher", "plain file\n");
    let error = fixture.check().unwrap_err();
    assert!(error.contains("staged") && error.contains("launcher"));
}

#[test]
fn detects_executable_bit_changes_in_the_index() {
    let fixture = Fixture::new();
    fixture.git(&["add", PATH]);
    fixture.git(&["commit", "-qm", "install registry"]);
    fixture.git(&["update-index", "--chmod=+x", "data"]);
    let error = fixture.check().unwrap_err();
    assert!(error.contains("staged") && error.contains("\"data\""));
}

#[test]
fn detects_package_targets_and_inline_workflow_commands() {
    let fixture = Fixture::new();
    fixture.write("package.json", r#"{"scripts":{"check":"node check.mjs"}}"#);
    fixture.write(
        ".github/workflows/new.yml",
        concat!(
            "jobs:\n  demo:\n    steps:\n",
            "      - {run: 'echo fixture'}\n",
            "      - \"run\": |\n          echo fixture\n          echo another\n",
        ),
    );
    let error = fixture.check().unwrap_err();
    assert!(error.contains("task \"check\""));
    assert_eq!(error.matches("demo:run:").count(), 2);
}

#[test]
fn detects_inline_github_script_and_preserves_duplicate_run_occurrences() {
    let fixture = Fixture::new();
    fixture.write(
        ".github/workflows/demo.yml",
        concat!(
            "jobs:\n  demo:\n    steps:\n",
            "      - run: echo fixture\n",
            "      - run: echo fixture\n",
            "      - uses: actions/github-script@v9\n",
            "        with: {script: 'return 1'}\n",
        ),
    );
    let error = fixture.check().unwrap_err();
    assert_eq!(error.matches("demo:run:").count(), 2);
    assert!(error.contains("demo:github-script:"));
}

#[test]
fn new_harness_registration_requires_a_catalogue_sensor() {
    let fixture = Fixture::new();
    fixture.write("check.sh", "echo checked\n");
    let mut registration = script("check.sh");
    registration["class"] = "harness".into();
    fixture.inventory(&[registration.clone()]);
    assert!(fixture.check().unwrap_err().contains("invalid entry"));
    registration["sensor"] = "repository".into();
    fixture.inventory(&[registration]);
    assert!(
        fixture
            .check()
            .unwrap_err()
            .contains("expected a harness manifest")
    );
    fixture.write(
        "harness/manifest.yaml",
        r#"{"sensors":[{"id":"repository","paired_guide":"guide"}]}"#,
    );
    fixture.check().unwrap();
}

#[test]
fn new_nested_cargo_alias_requires_exact_registration() {
    let fixture = Fixture::new();
    fixture.write("tools/.cargo/config.toml", "[alias]\nexample = 'fmt'\n");
    let error = fixture.check().unwrap_err();
    assert!(error.contains("tools/.cargo/config.toml") && error.contains("task \"example\""));
}

#[test]
fn registrations_must_accompany_a_concrete_executable() {
    let fixture = Fixture::new();
    fixture.inventory(&[script("future.sh")]);
    assert!(
        fixture
            .check()
            .unwrap_err()
            .contains("registration has no executable surface")
    );
}

#[cfg(unix)]
#[test]
fn does_not_read_tracked_files_through_a_replaced_directory() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = Fixture::new();
    fixture.write("nested/plain", "fixture\n");
    fixture.git(&["add", "nested/plain"]);
    fs::remove_dir_all(fixture.0.join("nested")).unwrap();
    outside.write("plain", "#!/bin/sh\n");
    symlink(&outside.0, fixture.0.join("nested")).unwrap();
    let error = fixture.check().unwrap_err();
    assert!(error.contains("linked or unreadable ancestor"));
}

#[test]
fn staged_shebang_is_checked_even_if_git_classifies_the_blob_as_binary() {
    let fixture = Fixture::new();
    fixture.git(&["add", PATH]);
    fixture.git(&["commit", "-qm", "install registry"]);
    fixture.write("launcher", "#!/bin/sh\n\0");
    fixture.git(&["add", "launcher"]);
    fixture.write("launcher", "plain file\n");
    let error = fixture.check().unwrap_err();
    assert!(error.contains("staged") && error.contains("launcher"));
}

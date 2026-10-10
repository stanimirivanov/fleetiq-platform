use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;

#[test]
fn current_repository_has_no_source_overrides() {
    let workspace_root = Path::new(super::super::WORKSPACE_MANIFEST)
        .parent()
        .expect("workspace manifest has a directory");

    check(workspace_root).expect("current Cargo policy has no overrides");
}

#[test]
fn missing_optional_configurations_are_allowed() {
    let fixture = RootFixture::new();

    check(&fixture.root).expect("only the root manifest is required");
}

#[test]
fn root_manifest_patch_rejects_path_git_and_registry_substitution() {
    for entry in [
        "sqlx = { path = '../local-database' }",
        "sqlx = { git = 'https://example.com/sqlx', rev = 'abc' }",
        "sqlx = { version = '0.9', registry = 'alternate' }",
    ] {
        let fixture = RootFixture::new();
        fixture.write("Cargo.toml", &format!("[patch.crates-io]\n{entry}\n"));

        assert_violation(&fixture, "Cargo [patch] overrides are not approved");
    }
}

#[test]
fn root_patch_supports_arbitrary_source_groups_without_exposing_source_urls() {
    let fixture = RootFixture::new();
    fixture.write(
        "Cargo.toml",
        "[patch.'https://private_marker:credential@example.com/dependency']\nvalue = { path = '../helper' }\n",
    );

    let violations = assert_violation(&fixture, "Cargo [patch] overrides are not approved");
    assert!(
        violations
            .iter()
            .all(|error| !error.contains("private_marker"))
    );
    assert!(violations.iter().all(|error| !error.contains("credential")));
}

#[test]
fn deprecated_root_replace_is_rejected() {
    let fixture = RootFixture::new();
    fixture.write(
        "Cargo.toml",
        "[replace]\n'sqlx:0.9.0' = { path = '../helper' }\n",
    );

    assert_violation(&fixture, "Cargo [replace] overrides are not approved");
}

#[test]
fn both_configuration_filenames_reject_path_overrides() {
    for filename in [".cargo/config.toml", ".cargo/config"] {
        let fixture = RootFixture::new();
        fixture.write(filename, "paths = ['../helper']\n");

        let violations = assert_violation(&fixture, "Cargo paths overrides are not approved");
        assert!(violations[0].contains(&fixture.root.join(filename).display().to_string()));
    }
}

#[test]
fn both_configuration_filenames_reject_patch_overrides() {
    for filename in [".cargo/config.toml", ".cargo/config"] {
        let fixture = RootFixture::new();
        fixture.write(
            filename,
            "[patch.crates-io]\nhelper = { path = '../helper' }\n",
        );

        assert_violation(&fixture, "Cargo [patch] overrides are not approved");
    }
}

#[test]
fn both_configuration_filenames_reject_source_replacement() {
    for filename in [".cargo/config.toml", ".cargo/config"] {
        let fixture = RootFixture::new();
        fixture.write(
            filename,
            "[source.crates-io]\nreplace-with = 'vendored'\n[source.vendored]\ndirectory = '../vendor'\n",
        );

        assert_violation(
            &fixture,
            "Cargo [source] replace-with overrides are not approved",
        );
    }
}

#[test]
fn legacy_configuration_is_checked_even_with_current_configuration_present() {
    let fixture = RootFixture::new();
    fixture.write(".cargo/config.toml", "[alias]\ncheck = 'check --locked'\n");
    fixture.write(".cargo/config", "paths = ['../helper']\n");

    assert_violation(&fixture, "Cargo paths overrides are not approved");
}

#[test]
fn empty_override_containers_are_allowed() {
    let fixture = RootFixture::new();
    fixture.write("Cargo.toml", "[patch]\n[patch.crates-io]\n[replace]\n");
    fixture.write(
        ".cargo/config.toml",
        "paths = []\n[patch]\n[patch.crates-io]\n[source]\n[source.crates-io]\n",
    );
    fixture.write(".cargo/config", "paths = []\n");

    check(&fixture.root).expect("empty containers do not override dependencies");
}

#[test]
fn comments_strings_and_nested_metadata_do_not_spoof_root_override_fields() {
    let fixture = RootFixture::new();
    fixture.write(
        "Cargo.toml",
        "# [patch.crates-io]\n# helper = { path = '../helper' }\n[workspace.metadata]\nexample = '''[replace]\nhelper = { path = '../helper' }'''\n[workspace.metadata.patch.crates-io]\nhelper = { path = '../helper' }\n",
    );
    fixture.write(
        ".cargo/config.toml",
        "# paths = ['../helper']\n[alias]\nexample = '''[source.crates-io]\nreplace-with = 'vendored' '''\n",
    );

    check(&fixture.root).expect("only parsed root policy fields count");
}

#[test]
fn malformed_policy_files_fail_without_echoing_file_contents() {
    for filename in ["Cargo.toml", ".cargo/config.toml", ".cargo/config"] {
        let fixture = RootFixture::new();
        fixture.write(filename, "private_marker = [\n");

        let violations = assert_violation(&fixture, "could not parse Cargo policy file as TOML");
        assert!(
            violations
                .iter()
                .all(|error| !error.contains("private_marker"))
        );
    }
}

#[test]
fn required_manifest_cannot_be_missing() {
    let fixture = RootFixture::new();
    fs::remove_file(fixture.root.join("Cargo.toml")).expect("fixture manifest should be removed");

    assert_violation(&fixture, "could not read Cargo policy file");
}

#[test]
fn unreadable_policy_paths_do_not_pass_as_optional_missing_files() {
    for filename in ["Cargo.toml", ".cargo/config.toml", ".cargo/config"] {
        let fixture = RootFixture::new();
        let path = fixture.root.join(filename);
        if path.is_file() {
            fs::remove_file(&path).expect("fixture file should be removed");
        }
        fs::create_dir(&path).expect("a directory cannot be read as a Cargo file");

        assert_violation(&fixture, "could not read Cargo policy file");
    }
}

#[test]
fn non_utf8_policy_files_fail_without_echoing_bytes() {
    let fixture = RootFixture::new();
    fs::write(fixture.root.join(".cargo/config.toml"), [0xff])
        .expect("fixture should contain invalid UTF-8");

    assert_violation(&fixture, "could not read Cargo policy file");
}

#[test]
fn malformed_override_shapes_are_rejected() {
    for (filename, source, expected) in [
        (
            "Cargo.toml",
            "patch = []\n",
            "Cargo [patch] must be a table",
        ),
        (
            "Cargo.toml",
            "replace = false\n",
            "Cargo [replace] must be a table",
        ),
        (
            ".cargo/config.toml",
            "paths = false\n",
            "Cargo paths must be an array",
        ),
        (
            "Cargo.toml",
            "[patch]\ncrates-io = false\n",
            "Cargo [patch] source entries must be tables",
        ),
        (
            ".cargo/config.toml",
            "source = false\n",
            "Cargo [source] must be a table",
        ),
        (
            ".cargo/config.toml",
            "[source]\ncrates-io = false\n",
            "Cargo [source] entries must be tables",
        ),
    ] {
        let fixture = RootFixture::new();
        fixture.write(filename, source);

        assert_violation(&fixture, expected);
    }
}

#[test]
fn all_root_policy_file_failures_are_aggregated_in_stable_order() {
    let fixture = RootFixture::new();
    fixture.write(
        "Cargo.toml",
        "[replace]\n'helper:1.0.0' = { path = '../helper' }\n",
    );
    fixture.write(".cargo/config.toml", "paths = ['../helper']\n");
    fixture.write(
        ".cargo/config",
        "[source.crates-io]\nreplace-with = 'vendored'\n",
    );

    let violations = check(&fixture.root).expect_err("all files violate source policy");
    assert_eq!(violations.len(), 3);
    assert!(violations.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(violations.iter().all(|error| error.contains(GUIDE)));
}

fn assert_violation(fixture: &RootFixture, expected: &str) -> Vec<String> {
    let violations = check(&fixture.root).expect_err("source policy should reject overrides");
    assert!(
        violations.iter().any(|error| error.contains(expected)),
        "expected {expected:?} in {violations:?}"
    );
    assert!(violations.iter().all(|error| error.contains(GUIDE)));
    violations
}

struct RootFixture {
    root: PathBuf,
}

impl RootFixture {
    fn new() -> Self {
        static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);
        loop {
            let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "fleetiq-source-overrides-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&root) {
                Ok(()) => {
                    fs::create_dir(root.join(".cargo"))
                        .expect("fixture Cargo directory should exist");
                    let fixture = Self { root };
                    fixture.write("Cargo.toml", "[workspace]\nmembers = []\n");
                    return fixture;
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => panic!("could not create source policy fixture: {error}"),
            }
        }
    }

    fn write(&self, filename: &str, source: &str) {
        fs::write(self.root.join(filename), source)
            .expect("fixture Cargo policy should be written");
    }
}

impl Drop for RootFixture {
    fn drop(&mut self) {
        // Remove only the exact paths owned by this fixture. Some tests replace
        // a policy file with a directory; neither cleanup path is recursive.
        for filename in ["Cargo.toml", ".cargo/config.toml", ".cargo/config"] {
            let path = self.root.join(filename);
            if fs::remove_file(&path).is_err() {
                let _ = fs::remove_dir(&path);
            }
        }
        let _ = fs::remove_dir(self.root.join(".cargo"));
        let _ = fs::remove_dir(&self.root);
    }
}

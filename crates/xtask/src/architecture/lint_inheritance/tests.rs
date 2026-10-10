use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;

#[test]
fn inherits_shared_lints_using_equivalent_toml_table_spellings() {
    for source in [
        "[lints]\nworkspace = true\n",
        "lints = { workspace = true }\n",
        "lints.workspace = true\n",
    ] {
        validate_manifest(source).expect("TOML meaning should determine inheritance");
    }
}

#[test]
fn missing_or_replaced_inheritance_is_rejected() {
    for (source, expected_guidance) in [
        (
            "[package]\nname = 'fixture'\n",
            "declare [lints] with workspace = true",
        ),
        (
            "[lints.rust]\nunsafe_code = 'forbid'\n",
            "[lints].workspace must be the boolean true",
        ),
        ("lints = false\n", "declare [lints] with workspace = true"),
    ] {
        let error =
            validate_manifest(source).expect_err("local rules do not inherit workspace lints");
        assert!(
            error.contains(expected_guidance),
            "unexpected guidance: {error}"
        );
    }
}

#[test]
fn false_and_mistyped_inheritance_are_rejected() {
    for value in ["false", "'true'", "1", "[]", "{}"] {
        let source = format!("[lints]\nworkspace = {value}\n");
        let error = validate_manifest(&source).expect_err("inheritance must be the boolean true");
        assert!(error.contains("must be the boolean true"));
    }
}

#[test]
fn comments_and_string_values_cannot_spoof_inheritance() {
    for source in [
        "# [lints]\n# workspace = true\n",
        "[package.metadata]\nexample = '''[lints]\nworkspace = true\n'''\n",
        "[package.metadata.lints]\nworkspace = true\n",
    ] {
        validate_manifest(source).expect_err("only the actual root lint table counts");
    }
}

#[test]
fn local_overrides_cannot_accompany_workspace_inheritance() {
    for local_policy in [
        "[lints.rust]\nunsafe_code = 'allow'\n",
        "[lints.clippy]\npedantic = 'allow'\n",
        "[lints.rustdoc]\nbroken_intra_doc_links = 'allow'\n",
    ] {
        let source = format!("[lints]\nworkspace = true\n{local_policy}");
        let error =
            validate_manifest(&source).expect_err("shared lint policy cannot be overridden");
        assert!(error.contains("remove package-local lint tables"));
    }
}

#[test]
fn malformed_toml_cannot_provide_passing_evidence_or_echo_manifest_content() {
    let error = validate_manifest("[lints]\nworkspace = true\nprivate_marker = [\n")
        .expect_err("a broken manifest cannot pass");
    assert!(error.contains("could not parse manifest as TOML"));
    assert!(!error.contains("private_marker"));
}

#[test]
fn manifest_diagnostic_names_the_package_path_and_policy() {
    let fixture = ManifestFixture::new();
    fs::write(&fixture.path, "[lints]\nworkspace = false\n").expect("fixture should be written");

    let error = check_manifest("fixture-package", &fixture.path)
        .expect_err("false inheritance should be rejected");

    assert!(error.contains("fixture-package"));
    assert!(error.contains(&fixture.path.display().to_string()));
    assert!(error.contains(GUIDE));
}

#[test]
fn unreadable_manifest_is_a_failed_check() {
    let fixture = ManifestFixture::new();

    let error = check_manifest("fixture-package", &fixture.path)
        .expect_err("a missing manifest cannot pass");

    assert!(error.contains("could not read manifest"));
    assert!(error.contains(GUIDE));
}

#[test]
fn non_utf8_manifest_is_a_failed_check() {
    let fixture = ManifestFixture::new();
    fs::write(&fixture.path, [0xff]).expect("invalid UTF-8 fixture should be written");

    let error = check_manifest("fixture-package", &fixture.path)
        .expect_err("a non-UTF-8 manifest cannot pass");

    assert!(error.contains("could not read manifest"));
}

#[test]
fn valid_manifest_can_be_read_and_checked() {
    let fixture = ManifestFixture::new();
    fs::write(&fixture.path, "[lints]\nworkspace = true\n").expect("fixture should be written");

    check_manifest("fixture-package", &fixture.path).expect("inherited lint policy should pass");
}

#[test]
fn member_failures_are_aggregated_in_stable_order() {
    let mut metadata = super::super::load_metadata().expect("workspace metadata should load");
    let fixtures = [ManifestFixture::new(), ManifestFixture::new()];
    let members = metadata.workspace_members[..fixtures.len()].to_vec();
    for (member, fixture) in members.iter().zip(&fixtures) {
        let package = metadata
            .packages
            .iter_mut()
            .find(|package| &package.id == member)
            .expect("Cargo-selected member should have package metadata");
        package.manifest_path = fixture
            .path
            .to_str()
            .expect("manifest fixtures should have UTF-8 paths")
            .into();
    }

    let errors = check(&metadata).expect_err("every unreadable member must be reported");
    let mut sorted = errors.clone();
    sorted.sort();

    assert_eq!(errors.len(), fixtures.len());
    assert_eq!(errors, sorted);
    for fixture in &fixtures {
        assert!(
            errors
                .iter()
                .any(|error| error.contains(&fixture.path.display().to_string()))
        );
    }
}
struct ManifestFixture {
    directory: PathBuf,
    path: PathBuf,
}

impl ManifestFixture {
    fn new() -> Self {
        static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);
        loop {
            let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir().join(format!(
                "fleetiq-lint-inheritance-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&directory) {
                Ok(()) => {
                    return Self {
                        path: directory.join("Cargo.toml"),
                        directory,
                    };
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("could not create manifest fixture: {error}"),
            }
        }
    }
}

impl Drop for ManifestFixture {
    fn drop(&mut self) {
        // This fixture owns one file and one directory; no recursive deletion
        // or filesystem traversal is required, including during test unwinding.
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_dir(&self.directory);
    }
}

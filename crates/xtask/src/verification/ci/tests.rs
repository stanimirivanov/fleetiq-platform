//! Workflow fixtures prove semantic weakening cannot hide behind matching text.

use std::{fs, path::Path};

use super::{TEMPLATE, check, check_document, render};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn template() -> String {
    fs::read_to_string(Path::new(ROOT).join(TEMPLATE)).expect("reviewed CI template")
}

fn generated() -> String {
    render(&template()).expect("valid CI placeholders")
}

#[test]
fn checked_in_workflow_matches_authoritative_inventory() {
    check(Path::new(ROOT)).expect("regenerate intentional CI changes with verify --write-ci");
}

#[test]
fn generated_document_and_crlf_are_accepted() {
    let expected = generated();
    assert_eq!(check_document(&expected, &expected), Ok(()));
    assert_eq!(
        check_document(&expected.replace('\n', "\r\n"), &expected),
        Ok(())
    );
}

#[test]
fn changed_runner_command_is_rejected() {
    let expected = generated();
    let changed = expected.replace("cargo xtask verify --group rust", "cargo fmt --all --check");
    assert_ne!(changed, expected);
    assert!(check_document(&changed, &expected).is_err());
}

#[test]
fn commented_runner_command_is_rejected() {
    let expected = generated();
    let changed = expected.replace(
        "        run: cargo xtask verify --group rust",
        "        # run: cargo xtask verify --group rust",
    );
    assert_ne!(changed, expected);
    assert!(check_document(&changed, &expected).is_err());
}

#[test]
fn conditional_or_tolerated_failures_are_rejected() {
    let expected = generated();
    for extra in ["        if: false", "        continue-on-error: true"] {
        let changed = expected.replace(
            "      - name: Run authoritative Rust verification",
            &format!("      - name: Run authoritative Rust verification\n{extra}"),
        );
        assert_ne!(changed, expected);
        assert!(check_document(&changed, &expected).is_err());
    }
}

#[test]
fn missing_group_and_modified_database_service_are_rejected() {
    let expected = generated();
    for changed in [
        expected.replace("        run: cargo xtask verify --group rust\n", ""),
        expected.replace("image: postgres:18.6-alpine3.23", "image: postgres:17"),
    ] {
        assert_ne!(changed, expected);
        assert!(check_document(&changed, &expected).is_err());
    }
}

#[test]
fn missing_repeated_and_unknown_placeholders_are_rejected() {
    let original = template();
    for changed in [
        original.replace("@verify:rust@", "cargo fmt --all --check"),
        format!("{original}\n@verify:rust@\n"),
        format!("{original}\n@tool:unknown@\n"),
    ] {
        assert!(render(&changed).is_err());
    }
}

#[test]
fn evidence_publication_cannot_be_removed_or_tolerate_missing_reports() {
    let expected = generated();
    for changed in [
        expected.replace("        if: always()\n", ""),
        expected.replace(
            "          if-no-files-found: error",
            "          if-no-files-found: ignore",
        ),
    ] {
        assert_ne!(changed, expected);
        assert!(check_document(&changed, &expected).is_err());
    }
}

#[test]
fn diagnostics_do_not_echo_workflow_content() {
    let expected = generated();
    let error = check_document("private-content-that-must-not-be-repeated", &expected)
        .expect_err("different workflow rejected");
    assert!(!error.contains("private-content"));
    assert!(error.contains(".github/workflows/ci.yml:1"));
    assert!(error.contains("verify --write-ci"));
}

struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "fleetiq-ci-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("create unique owned fixture directory");
        fs::create_dir(root.join(".github")).unwrap();
        fs::create_dir(root.join(".github/workflows")).unwrap();
        fs::write(root.join(TEMPLATE), template()).unwrap();
        fs::write(root.join(super::WORKFLOW), generated()).unwrap();
        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Cleanup names are confined to the newly created fixture; no recursive
        // removal follows redirects introduced by an adversarial fixture.
        for relative in [super::WORKFLOW, TEMPLATE, ".github/workflows", ".github"] {
            let candidate = self.0.join(relative);
            let _ = fs::remove_file(&candidate).or_else(|_| fs::remove_dir(&candidate));
        }
        let _ = fs::remove_dir(&self.0);
    }
}

#[test]
fn missing_non_file_and_invalid_utf8_inputs_fail_closed() {
    let missing = Fixture::new();
    fs::remove_file(missing.0.join(TEMPLATE)).unwrap();
    assert!(check(&missing.0).is_err());

    let non_file = Fixture::new();
    fs::remove_file(non_file.0.join(super::WORKFLOW)).unwrap();
    fs::create_dir(non_file.0.join(super::WORKFLOW)).unwrap();
    assert!(check(&non_file.0).is_err());

    let invalid_utf8 = Fixture::new();
    fs::write(invalid_utf8.0.join(TEMPLATE), [0xff, 0xfe]).unwrap();
    let error = check(&invalid_utf8.0).expect_err("invalid UTF-8 cannot be read as policy");
    assert!(error.contains("UTF-8"));
    assert!(!error.contains(invalid_utf8.0.to_str().unwrap()));
}

#[test]
fn explicit_regeneration_writes_only_the_canonical_workflow() {
    let fixture = Fixture::new();
    let original_template = fs::read(fixture.0.join(TEMPLATE)).unwrap();
    fs::write(fixture.0.join(super::WORKFLOW), "drifted").unwrap();
    assert!(check(&fixture.0).is_err());
    super::write(&fixture.0).unwrap();
    check(&fixture.0).unwrap();
    assert_eq!(
        fs::read(fixture.0.join(TEMPLATE)).unwrap(),
        original_template
    );
    super::write(&fixture.0).unwrap();
    check(&fixture.0).unwrap();
}

#[cfg(unix)]
#[test]
fn policy_file_symlinks_are_rejected_without_reading_or_overwriting_targets() {
    let fixture = Fixture::new();
    let target = fixture.0.join(TEMPLATE);
    let original = fs::read(&target).unwrap();
    fs::remove_file(fixture.0.join(super::WORKFLOW)).unwrap();
    std::os::unix::fs::symlink(&target, fixture.0.join(super::WORKFLOW)).unwrap();
    assert!(check(&fixture.0).is_err());
    assert!(super::write(&fixture.0).is_err());
    assert_eq!(fs::read(target).unwrap(), original);
}

#[test]
fn windows_sensor_coverage_cannot_be_disabled_or_replaced() {
    let expected = generated();
    assert!(expected.contains("name: Windows harness sensors"));
    assert!(expected.contains("runs-on: windows-latest"));
    for changed in [
        expected.replace("runs-on: windows-latest", "runs-on: ubuntu-latest"),
        expected.replace(
            "        run: cargo test -p fleetiq-xtask --locked",
            "        run: cargo --version",
        ),
        expected.replace(
            "      - name: Exercise Windows repository sensor fixtures",
            "      - name: Exercise Windows repository sensor fixtures\n        if: false",
        ),
    ] {
        assert_ne!(changed, expected);
        assert!(check_document(&changed, &expected).is_err());
    }
}

#[cfg(any(unix, windows))]
#[test]
fn redirected_policy_parent_cannot_be_read_or_regenerated() {
    let fixture = Fixture::new();
    let outside = Fixture::new();
    check(&fixture.0).expect("ordinary policy files are accepted");
    let template_before = fs::read(outside.0.join(TEMPLATE)).unwrap();
    let workflow_before = fs::read(outside.0.join(super::WORKFLOW)).unwrap();

    for relative in [super::WORKFLOW, TEMPLATE] {
        fs::remove_file(fixture.0.join(relative)).unwrap();
    }
    fs::remove_dir(fixture.0.join(".github").join("workflows")).unwrap();
    fs::remove_dir(fixture.0.join(".github")).unwrap();
    let link = fixture.0.join(".github");
    let target = outside.0.join(".github");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    #[cfg(windows)]
    {
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "policy junction setup must succeed");
    }
    // Remove only this owned redirect before either fixture's cleanup runs.
    let _redirect = PolicyParentLink(link);
    let error = check(&fixture.0).expect_err("redirected policy fails closed");
    assert!(error.contains("symlink or junction"));
    assert!(super::write(&fixture.0).is_err());
    assert_eq!(fs::read(outside.0.join(TEMPLATE)).unwrap(), template_before);
    assert_eq!(
        fs::read(outside.0.join(super::WORKFLOW)).unwrap(),
        workflow_before
    );
}

#[cfg(any(unix, windows))]
struct PolicyParentLink(std::path::PathBuf);

#[cfg(any(unix, windows))]
impl Drop for PolicyParentLink {
    fn drop(&mut self) {
        #[cfg(unix)]
        fs::remove_file(&self.0).expect("remove only the owned policy symlink");
        #[cfg(windows)]
        fs::remove_dir(&self.0).expect("remove only the owned policy junction");
    }
}

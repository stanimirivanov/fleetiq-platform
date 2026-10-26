//! Workflow fixtures prove semantic weakening cannot hide behind matching text.

use std::{fs, path::Path};

use super::{TEMPLATE, check, check_document, render};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn template() -> String {
    fs::read_to_string(Path::new(ROOT).join(TEMPLATE)).expect("reviewed CI template")
}

fn workflow() -> String {
    fs::read_to_string(Path::new(ROOT).join(super::WORKFLOW)).expect("workflow action tags")
}

fn generated() -> String {
    render(&template(), &workflow()).expect("valid CI placeholders")
}

#[test]
fn checked_in_workflow_matches_authoritative_inventory() {
    check(Path::new(ROOT)).expect("regenerate intentional CI changes with verify --write-ci");
}

#[test]
fn pull_requests_are_the_only_ci_event_and_supply_the_trusted_base() {
    let expected = generated();
    let events = expected
        .split_once("\non:\n")
        .unwrap()
        .1
        .split_once("\npermissions:\n")
        .unwrap()
        .0;
    assert_eq!(events, "  pull_request:\n");
    assert!(expected.contains("HARNESS_BASE: ${{ github.event.pull_request.base.sha }}"));
    assert!(!expected.contains("github.event.before"));
}

#[test]
fn trigger_and_trusted_base_drift_are_rejected() {
    let fixture = Fixture::new();
    let original = generated();
    let trigger = "on:\n  pull_request:\n";
    for changed in [
        original.replacen(trigger, "on:\n  push:\n    branches: [main]\n", 1),
        original.replacen(
            trigger,
            "on:\n  pull_request:\n  push:\n    branches: [main]\n",
            1,
        ),
        original.replacen(trigger, "on:\n  pull_request:\n  workflow_dispatch:\n", 1),
        original.replacen(
            trigger,
            "on:\n  pull_request:\n  schedule:\n    - cron: '0 0 * * *'\n",
            1,
        ),
        original.replacen(trigger, "on:\n  pull_request:\n    branches: [main]\n", 1),
        original.replace("github.event.pull_request.base.sha", "github.event.before"),
        original.replace("${{ github.event.pull_request.base.sha }}", "HEAD^"),
    ] {
        assert_ne!(changed, original);
        fs::write(fixture.0.join(super::WORKFLOW), changed).unwrap();
        assert!(
            check(&fixture.0).is_err(),
            "event or trusted-base drift must fail"
        );
    }
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
fn verification_jobs_are_independent_and_migrations_use_the_pinned_cli() {
    let expected = generated();
    let architecture = expected
        .split_once("\n  architecture:\n")
        .unwrap()
        .1
        .split_once("\n  harness-windows:\n")
        .unwrap()
        .0;
    let auxiliary_setup = architecture
        .find("Install Node.js for the checksummed scanner")
        .unwrap();
    for command in ["cargo xtask repository", "cargo xtask architecture"] {
        assert!(
            architecture.find(command).unwrap() < auxiliary_setup,
            "{command} must run before the scanner's Node setup"
        );
    }
    let jobs = [
        ("architecture", "harness-windows"),
        ("harness-windows", "openapi"),
        ("openapi", "supply-chain"),
        ("supply-chain", "rust"),
        ("rust", "local-compose"),
    ];
    for (job, next) in jobs {
        let section = expected
            .split_once(&format!("  {job}:\n"))
            .unwrap_or_else(|| panic!("missing {job} job"))
            .1
            .split_once(&format!("\n  {next}:\n"))
            .unwrap_or_else(|| panic!("missing {next} job"))
            .0;
        assert!(
            !section
                .lines()
                .any(|line| line.trim().starts_with("needs:")),
            "{job} must execute independently of another job's setup"
        );
    }
    let compose = expected.split_once("\n  local-compose:\n").unwrap().1;
    assert!(
        !compose
            .lines()
            .any(|line| line.trim().starts_with("needs:"))
    );
    let rust = expected
        .split_once("\n  rust:\n")
        .unwrap()
        .1
        .split_once("\n  local-compose:\n")
        .unwrap()
        .0;
    assert!(rust.contains("run: sqlx migrate run --source migrations"));
    assert!(!rust.contains("psql "));
}

#[test]
fn missing_repeated_and_unknown_placeholders_are_rejected() {
    let original = template();
    for changed in [
        original.replace("@verify:rust@", "cargo fmt --all --check"),
        format!("{original}\n@verify:rust@\n"),
        format!("{original}\n@tool:unknown@\n"),
    ] {
        assert!(render(&changed, &workflow()).is_err());
    }
}

#[test]
fn evidence_publication_cannot_be_removed_or_tolerate_missing_reports() {
    let expected = generated();
    for changed in [
        expected.replace(
            "        if: always() && steps.verify.outcome != 'skipped'\n",
            "",
        ),
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
    let changed = generated().replace("contents: read", "contents: write");
    fs::write(fixture.0.join(super::WORKFLOW), changed).unwrap();
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
    assert!(expected.contains("runs-on: windows-2025"));
    for changed in [
        expected.replace("runs-on: windows-2025", "runs-on: ubuntu-26.04"),
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

// A synthetic major-version update is used only in owned filesystem fixtures.
fn action_candidate(document: &str) -> String {
    let previous = document
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("uses: actions/setup-node@"))
        .expect("setup-node fixture tag");
    let next_major = previous
        .strip_prefix('v')
        .expect("major-version tag")
        .parse::<u32>()
        .expect("numeric major version")
        + 1;
    document.replace(
        &format!("uses: actions/setup-node@{previous}"),
        &format!("uses: actions/setup-node@v{next_major}"),
    )
}

#[test]
fn major_tag_only_update_passes_without_template_edit_or_regeneration() {
    let fixture = Fixture::new();
    let path = fixture.0.join(super::WORKFLOW);
    let original_template = fs::read(fixture.0.join(TEMPLATE)).unwrap();
    let candidate = action_candidate(&generated());
    fs::write(&path, &candidate).unwrap();

    check(&fixture.0).expect("workflow-only major-version update is supported");
    assert_eq!(
        fs::read(fixture.0.join(TEMPLATE)).unwrap(),
        original_template
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), candidate);
    super::write(&fixture.0).unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), candidate);
}

#[test]
fn dependency_update_cannot_hide_changed_permissions_commands_or_setup() {
    let fixture = Fixture::new();
    let candidate = action_candidate(&generated());
    for changed in [
        candidate.replace("contents: read", "contents: write"),
        candidate.replace("persist-credentials: false", "persist-credentials: true"),
        candidate.replace(
            "cache-dependency-path: pnpm-lock.yaml",
            "cache-dependency-path: other.lock",
        ),
        candidate.replace(
            "package-manager-cache: false",
            "package-manager-cache: true",
        ),
        candidate.replace("node-version: '24'", "node-version: '25'"),
        candidate.replace("cargo xtask verify --group rust", "cargo --version"),
        candidate.replace(
            "      - name: Run authoritative Rust verification",
            "      - name: Run authoritative Rust verification\n        continue-on-error: true",
        ),
    ] {
        assert_ne!(changed, candidate);
        fs::write(fixture.0.join(super::WORKFLOW), changed).unwrap();
        assert!(check(&fixture.0).is_err());
    }
}

#[test]
fn malformed_inconsistent_and_substituted_actions_fail_without_repair() {
    let fixture = Fixture::new();
    let original = generated();
    let prefix = "uses: actions/setup-node@";
    let tag = original
        .lines()
        .find_map(|line| line.trim_start().strip_prefix(prefix))
        .unwrap();
    let full_reference = format!("{prefix}{tag}");
    let next_tag = action_candidate(&original)
        .lines()
        .find_map(|line| line.trim_start().strip_prefix(prefix))
        .unwrap()
        .to_owned();
    for invalid in [
        "",
        "7",
        "v",
        "v0",
        "v07",
        "v7.1.0",
        "v7 # release",
        "v7-branch",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    ] {
        let changed = original.replace(&full_reference, &format!("{prefix}{invalid}"));
        assert_ne!(changed, original);
        let path = fixture.0.join(super::WORKFLOW);
        fs::write(&path, &changed).unwrap();
        assert!(check(&fixture.0).is_err());
        assert!(super::write(&fixture.0).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), changed);
    }
    for changed in [
        original.replacen(&full_reference, &format!("{prefix}{next_tag}"), 1),
        original.replace(
            &full_reference,
            &format!("{full_reference}\n        if: false"),
        ),
        original.replace(&full_reference, &format!("# {full_reference}")),
        original.replace(
            &full_reference,
            &format!("uses: unapproved/setup-node@{tag}"),
        ),
    ] {
        assert_ne!(changed, original);
        let path = fixture.0.join(super::WORKFLOW);
        fs::write(&path, &changed).unwrap();
        assert!(check(&fixture.0).is_err());
        // Structural injection with otherwise valid refs is repairable explicitly;
        // invalid or inconsistent refs must never be silently reset.
        if !changed.contains("        if: false") && !changed.contains("# uses:") {
            assert!(super::write(&fixture.0).is_err());
            assert_eq!(fs::read_to_string(path).unwrap(), changed);
        }
    }
}

#[test]
fn action_slots_and_crlf_workflow_use_the_same_policy() {
    let fixture = Fixture::new();
    fs::write(
        fixture.0.join(super::WORKFLOW),
        generated().replace('\n', "\r\n"),
    )
    .unwrap();
    check(&fixture.0).unwrap();
    for changed in [
        template().replace("@action:setup-node@", "@action:unknown@"),
        template().replace("@action:setup-node@", "unowned-pin"),
    ] {
        fs::write(fixture.0.join(TEMPLATE), changed).unwrap();
        assert!(check(&fixture.0).is_err());
    }
}

#[test]
fn rust_and_supply_chain_tools_do_not_require_node_and_database_env_is_local() {
    use crate::verification::catalogue::{CHECKS, Group, Tool};

    for check in CHECKS
        .iter()
        .filter(|check| matches!(check.group, Group::Rust | Group::SupplyChain))
    {
        assert!(!check.tools.contains(&Tool::Node));
        assert!(!check.tools.contains(&Tool::Pnpm));
    }
    let workflow = generated();
    assert_eq!(workflow.matches("DATABASE_URL:").count(), 1);
    assert!(
        !workflow
            .split("jobs:")
            .next()
            .unwrap()
            .contains("DATABASE_URL")
    );
    for job in ["rust", "supply-chain"] {
        let marker = format!("  {job}:\n");
        let section = workflow.split(&marker).nth(1).unwrap();
        let body = section.split("\n  local-compose:").next().unwrap();
        assert!(!body.contains("uses: actions/setup-node@"));
        assert!(!body.contains("uses: pnpm/action-setup@"));
    }
}

#[test]
fn shell_literal_action_text_is_opaque_to_dependency_ref_collection() {
    let fixture = Fixture::new();
    let marker = "      - name: Install Node.js for the checksummed scanner installer";
    // The full template comparison owns this block. Its payload is shell data,
    // not another step, even though it contains YAML-looking action text.
    let block = "      - name: Shell literal fixture\n        run: |\n          cat <<'FIXTURE'\n          uses: unapproved/fixture@not-an-action-ref\n          FIXTURE\n\n";
    let with_block = |document: String| {
        assert!(document.contains(marker));
        document.replace(marker, &format!("{block}{marker}"))
    };
    fs::write(fixture.0.join(TEMPLATE), with_block(template())).unwrap();
    let candidate = action_candidate(&with_block(generated()));
    fs::write(fixture.0.join(super::WORKFLOW), &candidate).unwrap();
    check(&fixture.0).expect("opaque shell text does not supply action dependency refs");
    super::write(&fixture.0).unwrap();
    assert_eq!(
        fs::read_to_string(fixture.0.join(super::WORKFLOW)).unwrap(),
        candidate
    );
}

#[test]
fn performance_actions_remain_workflow_owned_major_dependencies() {
    for repository in ["Swatinem/rust-cache", "taiki-e/install-action"] {
        let fixture = Fixture::new();
        let original = generated();
        let prefix = format!("uses: {repository}@");
        let tag = original
            .lines()
            .find_map(|line| line.trim_start().strip_prefix(&prefix))
            .unwrap();
        let major = tag.strip_prefix('v').unwrap().parse::<u32>().unwrap() + 1;
        let changed = original.replace(&format!("{prefix}{tag}"), &format!("{prefix}v{major}"));
        fs::write(fixture.0.join(super::WORKFLOW), &changed).unwrap();
        check(&fixture.0).expect("action-only updates preserve reviewed structure");
        super::write(&fixture.0).unwrap();
        assert_eq!(
            fs::read_to_string(fixture.0.join(super::WORKFLOW)).unwrap(),
            changed
        );
    }
}
#[test]
fn caching_and_nextest_cannot_bypass_checks_or_change_reviewed_setup() {
    let fixture = Fixture::new();
    let original = generated();
    for changed in [
        original.replace("checksum: true","checksum: false"),
        original.replace("fallback: none","fallback: cargo-install"),
        original.replace("cargo-nextest@0.9.145","cargo-nextest@0.9.144"),
        original.replace("cache-bin: false","cache-bin: true"),
        original.replace("cache-targets: false","cache-targets: true"),
        original.replace("prefix-key: fleetiq-rust-v1","shared-key: shared"),
        original.replace("target/debug/deps","target/verification"),
        original.replace("cache-workspace-crates: false","cache-workspace-crates: true"),
        original.replace("cache-on-failure: false","cache-on-failure: true"),
        original.replace("github.event_name == 'push' && github.ref == 'refs/heads/main'","true"),
        original.replace("'Cargo.lock', '.config/nextest.toml'","'Cargo.toml'"),
        original.replace("env-vars: RUSTFLAGS RUSTDOCFLAGS CARGO_BUILD_TARGET","env-vars: IGNORED"),
        original.replace("        id: verify\n        run: cargo xtask verify --group rust","        id: verify\n        if: steps.rust-cache.outputs.cache-hit != 'true'\n        run: cargo xtask verify --group rust"),
    ] {
        assert_ne!(changed,original);
        fs::write(fixture.0.join(super::WORKFLOW),changed).unwrap();
        assert!(check(&fixture.0).is_err(),"setup weakening must fail");
    }
}

#[test]
fn rust_evidence_retains_measured_ingestion_reports() {
    let fixture = Fixture::new();
    let original = generated();
    for changed in [
        original.replace("            target/ingestion-measurements/*.json\n", ""),
        original.replace(
            "target/ingestion-measurements/*.json",
            "target/missing-measurements/*.json",
        ),
    ] {
        assert_ne!(changed, original);
        fs::write(fixture.0.join(super::WORKFLOW), changed).unwrap();
        assert!(
            check(&fixture.0).is_err(),
            "measurement evidence retention must remain reviewed"
        );
    }
}

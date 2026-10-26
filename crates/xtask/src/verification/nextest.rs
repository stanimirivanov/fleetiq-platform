//! Reviewed Nextest execution shape and isolation from ambient runner options.
//!
//! Keep the runtime TOML readable. Structural equality of parsed values rejects
//! hidden selection, retries, scripts and extra profiles without interpreting
//! Nextest's filter language or writing a second scheduler.
use super::{catalogue::Tool, ci};
use std::{env, path::Path, process::Command};

pub(super) fn check(root: &Path) -> Result<(), String> {
    validate(&ci::read_regular(root, ".config/nextest.toml")?)
}
fn validate(document: &str) -> Result<(), String> {
    let actual = document.parse::<toml::Table>();
    let expected = expected()
        .parse::<toml::Table>()
        .expect("valid reviewed Nextest policy");
    if actual.as_ref().is_ok_and(|value| *value == expected) {
        Ok(())
    } else {
        Err(".config/nextest.toml differs from the reviewed complete-suite policy; retain eight global slots, four PostgreSQL slots, two Wasmtime slots, zero retries and no selection overrides; policy: docs/development/verification.md".into())
    }
}
fn expected() -> String {
    format!(
        r#"nextest-version = "{}"
[profile.default]
test-threads = 8
fail-fast = false
retries = 0
[profile.ci]
test-threads = 8
fail-fast = false
retries = 0
[test-groups]
postgres = {{ max-threads = 4 }}
wasmtime = {{ max-threads = 2 }}
[[profile.default.overrides]]
filter = 'package(fleetiq-server) & test(/^tcp_ingestion::tests::measured::measured_bounded_tcp_envelope$/)'
test-group = "postgres"
threads-required = 8
[[profile.default.overrides]]
filter = 'package(fleetiq-postgres) | package(fleetiq-server)'
test-group = "postgres"
[[profile.default.overrides]]
filter = 'package(fleetiq-protocol-wasm)'
test-group = "wasmtime"
"#,
        Tool::Nextest.expected()
    )
}
/// The catalogue and reviewed profile own execution, not shell runner overrides.
/// Retain Cargo/database configuration and never print environment values.
pub(super) fn isolate(command: &mut Command) {
    for (name, _) in env::vars_os() {
        if name
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("NEXTEST_")
        {
            command.env_remove(name);
        }
    }
    // Include explicit command overrides in tests without mutating global state.
    for name in [
        "NEXTEST_RETRIES",
        "NEXTEST_TEST_THREADS",
        "NEXTEST_PROFILE",
        "NEXTEST_RUN_EXTRA_ARGS",
    ] {
        command.env_remove(name);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reviewed_config_and_comments_are_accepted() {
        validate(&expected()).unwrap();
        validate(&format!("# comment\n{}", expected().replace('\n', "\r\n"))).unwrap();
        check(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))).unwrap();
    }
    #[test]
    fn filters_unbounded_resources_retries_and_renamed_groups_fail_closed() {
        let original = expected();
        for changed in [
            original.replace("test-threads = 8", "test-threads = 32"),
            original.replace("max-threads = 4", "max-threads = 20"),
            original.replace("max-threads = 2", "max-threads = 8"),
            original.replace("retries = 0", "retries = 1"),
            original.replace("threads-required = 8", "threads-required = 1"),
            original.replace(
                "test(/^tcp_ingestion::tests::measured::measured_bounded_tcp_envelope$/)",
                "test(/^different_measurement$/)",
            ),
            original.replace("fail-fast = false", "fail-fast = true"),
            original.replace("package(fleetiq-server)", "package(no-server-tests)"),
            original.replace("test-group = \"postgres\"", "test-group = \"wasmtime\""),
            original.replace("[profile.ci]", "[profile.ci]\ndefault-filter = 'none()'"),
            original.replace(
                "[profile.ci]",
                "[profile.ci]\nrun-extra-args = ['--skip', 'concurrent']",
            ),
            original.replace("0.9.145", "0.9.144"),
            format!("{original}\n[profile.hidden]\nretries = 5\n"),
            "invalid private configuration must not be echoed".into(),
        ] {
            assert_ne!(changed, original);
            let error = validate(&changed).expect_err("policy drift rejected");
            assert!(error.contains(".config/nextest.toml"));
            assert!(!error.contains("private configuration"));
        }
    }
    #[test]
    fn ambient_options_cannot_override_required_execution() {
        let mut command = Command::new("cargo");
        command
            .env("NEXTEST_RETRIES", "5")
            .env("NEXTEST_TEST_THREADS", "128")
            .env("NEXTEST_PROFILE", "hidden")
            .env("NEXTEST_RUN_EXTRA_ARGS", "--skip concurrency")
            .env("DATABASE_URL", "fixture-database");
        isolate(&mut command);
        let overrides: std::collections::BTreeMap<_, _> = command.get_envs().collect();
        for name in [
            "NEXTEST_RETRIES",
            "NEXTEST_TEST_THREADS",
            "NEXTEST_PROFILE",
            "NEXTEST_RUN_EXTRA_ARGS",
        ] {
            assert_eq!(overrides[std::ffi::OsStr::new(name)], None);
        }
        assert_eq!(
            overrides[std::ffi::OsStr::new("DATABASE_URL")],
            Some(std::ffi::OsStr::new("fixture-database"))
        );
    }

    #[test]
    fn missing_and_redirected_configuration_never_passes() {
        use std::fs;
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "fleetiq-nextest-policy-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let owned = ConfigFixture(root);
        assert!(check(&owned.0).is_err());
        fs::create_dir(owned.0.join(".config")).unwrap();
        let config = owned.0.join(".config/nextest.toml");
        fs::write(&config, expected()).unwrap();
        check(&owned.0).unwrap();
        fs::write(&config, [0xff]).unwrap();
        assert!(check(&owned.0).is_err());
        fs::write(&config, expected()).unwrap();
        fs::create_dir(owned.0.join("policy")).unwrap();
        fs::rename(&config, owned.0.join("policy/nextest.toml")).unwrap();
        fs::remove_dir(owned.0.join(".config")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(owned.0.join("policy"), owned.0.join(".config")).unwrap();
        #[cfg(windows)]
        {
            let status = Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(owned.0.join(".config"))
                .arg(owned.0.join("policy"))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "junction fixture must succeed");
        }
        #[cfg(any(unix, windows))]
        {
            let error = check(&owned.0).expect_err("redirected policy must fail");
            assert!(error.contains("symlink or junction"));
        }
    }
    struct ConfigFixture(std::path::PathBuf);
    impl Drop for ConfigFixture {
        fn drop(&mut self) {
            // Never recurse through an adversarial policy redirect.
            #[cfg(unix)]
            {
                let _ = std::fs::remove_file(self.0.join(".config"));
            }
            #[cfg(windows)]
            {
                let path = self.0.join(".config");
                if std::fs::symlink_metadata(&path).is_ok_and(|m| {
                    use std::os::windows::fs::MetadataExt;
                    m.file_attributes() & 0x400 != 0
                }) {
                    let _ = std::fs::remove_dir(path);
                }
            }
            for relative in [".config/nextest.toml", "policy/nextest.toml"] {
                let _ = std::fs::remove_file(self.0.join(relative));
            }
            for relative in [".config", "policy"] {
                let _ = std::fs::remove_dir(self.0.join(relative));
            }
            let _ = std::fs::remove_dir(&self.0);
        }
    }
}

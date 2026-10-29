use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;

static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);
const FIRST: &str = "migrations/0001_foundation.sql";
const SECOND: &str = "migrations/0002_extension.sql";

#[test]
fn unchanged_chain_reports_exact_baseline_and_counts() {
    let fixture = Fixture::new();
    let report = fixture.check().expect("unchanged published chain passes");
    assert_eq!(report.base_commit(), fixture.base);
    assert_eq!(report.immutable_count(), 2);
    assert_eq!(report.added_count(), 0);
}

#[test]
fn staged_and_untracked_forward_migrations_are_counted() {
    let fixture = Fixture::new();
    fixture.write("migrations/0003_next.sql", b"SELECT 3;\n");
    fixture.git(&["add", "--", "migrations/0003_next.sql"]);
    fixture.write("migrations/0010_later.sql", b"SELECT 10;\n");
    let report = fixture.check().expect("new unique forward versions pass");
    assert_eq!(report.added_count(), 2);
    assert_eq!(report.immutable_count(), 2);
}

#[test]
fn a_forward_migration_committed_after_the_baseline_passes() {
    let fixture = Fixture::new();
    fixture.write("migrations/0003_next.sql", b"SELECT 3;\n");
    fixture.commit();
    assert_eq!(
        fixture
            .check()
            .expect("new commit is a forward append")
            .added_count(),
        1
    );
}

#[test]
fn published_bytes_are_immutable_even_for_comments_and_whitespace() {
    for content in [
        b"SELECT 99;\n".as_slice(),
        b"-- revised comment\nSELECT 1;\n",
        b"SELECT 1; \n",
        b"SELECT 1;\r\n",
    ] {
        let fixture = Fixture::new();
        fixture.write(FIRST, content);
        fixture.rejects("has changed bytes");
    }
}

#[test]
fn staged_content_cannot_be_hidden_by_restoring_only_the_working_file() {
    let fixture = Fixture::new();
    fixture.write(FIRST, b"SELECT 999;\n");
    fixture.git(&["add", "--", FIRST]);
    fixture.write(FIRST, b"SELECT 1;\n");
    fixture.rejects("staged content or mode change");
}

#[test]
fn published_deletion_and_rename_are_rejected() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.root.join(FIRST)).expect("fixture file can be removed");
    fixture.rejects("is missing");
    fixture.write("migrations/0003_renamed.sql", b"SELECT 1;\n");
    fixture.rejects("instead of deleting or renaming history");
}

#[test]
fn staged_deletion_cannot_be_hidden_by_an_untracked_copy() {
    let fixture = Fixture::new();
    fixture.git(&["rm", "--", FIRST]);
    fixture.write(FIRST, b"SELECT 1;\n");
    fixture.rejects("absent from the Git index");
}

#[test]
fn staged_executable_mode_is_rejected_on_every_platform() {
    let fixture = Fixture::new();
    fixture.git(&["update-index", "--chmod=+x", "--", FIRST]);
    fixture.rejects("has Git mode 100755");
}

#[test]
fn staged_new_file_missing_from_disk_is_rejected() {
    let fixture = Fixture::new();
    let path = "migrations/0003_next.sql";
    fixture.write(path, b"SELECT 3;\n");
    fixture.git(&["add", "--", path]);
    fs::remove_file(fixture.root.join(path)).expect("staged fixture file can be removed");
    fixture.rejects("missing from the working tree");
}

#[test]
fn duplicate_versions_are_rejected_even_with_different_padding() {
    let fixture = Fixture::new();
    fixture.write("migrations/0003_one.sql", b"SELECT 3;\n");
    fixture.write("migrations/00003_other.sql", b"SELECT 3;\n");
    fixture.rejects("share numeric version 3");
}

#[test]
fn inserting_into_a_published_gap_is_rejected() {
    let fixture = Fixture::new();
    fixture.write("migrations/0005_future.sql", b"SELECT 5;\n");
    let base = fixture.commit();
    fixture.write("migrations/0004_backfilled.sql", b"SELECT 4;\n");
    let error = check_at(&fixture.root, &base).expect_err("published gaps cannot be backfilled");
    assert!(error.to_string().contains("high-water mark 5"));
}

#[test]
fn reusing_a_published_version_is_rejected() {
    let fixture = Fixture::new();
    fixture.write("migrations/0002_another.sql", b"SELECT 2;\n");
    fixture.rejects("at or below published high-water mark 2");
}

#[test]
fn malformed_names_and_out_of_range_versions_are_rejected() {
    for name in [
        "3_short.sql",
        "0000_zero.sql",
        "0003_Mixed.sql",
        "0003_two__words.sql",
        "0003_bad-name.sql",
        "0003_.sql",
        "0003_next.SQL",
        "0003_next.sql.bak",
        "0003_3start.sql",
        "999999999999999999999999_next.sql",
        "README.md",
    ] {
        let fixture = Fixture::new();
        fixture.write(&format!("migrations/{name}"), b"SELECT 3;\n");
        fixture.rejects("invalid migration");
    }
}

#[test]
fn malformed_or_duplicate_published_versions_are_rejected() {
    for path in ["migrations/0002_other.sql", "migrations/0003_Invalid.sql"] {
        let fixture = Fixture::new();
        fixture.write(path, b"SELECT 3;\n");
        let base = fixture.commit();
        let error = check_at(&fixture.root, &base)
            .expect_err("invalid published inventories cannot become passing evidence");
        assert!(error.to_string().contains(GUIDE));
    }
}

#[test]
fn executable_mode_in_the_baseline_is_rejected() {
    let fixture = Fixture::new();
    fixture.git(&["update-index", "--chmod=+x", "--", FIRST]);
    fixture.git(&[
        "commit",
        "--quiet",
        "-m",
        "invalid executable migration fixture",
    ]);
    let base = String::from_utf8(fixture.git(&["rev-parse", "HEAD"]).stdout)
        .expect("commit ID is UTF-8")
        .trim()
        .to_owned();
    let error = check_at(&fixture.root, &base)
        .expect_err("published history must consist of ordinary SQL files");
    assert!(error.to_string().contains("unsupported Git mode 100755"));
}

#[test]
fn nested_directory_is_rejected() {
    let fixture = Fixture::new();
    fixture.write("migrations/nested/0003_next.sql", b"SELECT 3;\n");
    fixture.rejects("remove nested directories");
}

#[test]
fn missing_migrations_directory_fails_without_empty_history_assumption() {
    let fixture = Fixture::new();
    let original = fixture.root.join("migrations");
    fs::rename(&original, fixture.root.join("removed-migrations"))
        .expect("fixture directory can be renamed");
    fixture.rejects("read migrations directory");
}

#[test]
fn empty_missing_and_option_like_baselines_do_not_pass() {
    let fixture = Fixture::new();
    for base in ["", " ", "missing-ref", "--all"] {
        let error = check_at(&fixture.root, base).expect_err("baseline must resolve explicitly");
        assert!(error.to_string().contains(GUIDE));
    }
}

#[test]
fn baseline_without_a_published_chain_is_rejected() {
    let fixture = Fixture::empty();
    fixture.write("README.md", b"fixture\n");
    let base = fixture.commit();
    fixture.write(FIRST, b"SELECT 1;\n");
    let error =
        check_at(&fixture.root, &base).expect_err("empty baselines are not trusted history");
    assert!(error.to_string().contains("has no published migrations"));
}

#[test]
fn a_ref_resolves_to_the_published_commit_without_shell_interpolation() {
    let fixture = Fixture::new();
    fixture.git(&["branch", "trusted-published", &fixture.base]);
    let report = check_at(&fixture.root, "trusted-published").expect("explicit ref is supported");
    assert_eq!(report.base_commit(), fixture.base);
}

#[test]
fn nonancestor_baseline_is_rejected() {
    let fixture = Fixture::new();
    fixture.git(&["checkout", "--orphan", "unrelated"]);
    // An orphan with the same tree/message/timestamp can reproduce the original
    // commit ID. Distinct content makes unrelated history deterministic.
    fixture.write("migrations/0003_unrelated.sql", b"SELECT 3;\n");
    let unrelated = fixture.commit();
    fixture.git(&["checkout", "--detach", &fixture.base]);
    let error =
        check_at(&fixture.root, &unrelated).expect_err("unrelated history is not a baseline");
    assert!(error.to_string().contains("not an ancestor of HEAD"));
}

#[test]
fn shallow_clone_missing_the_baseline_fails_closed() {
    let fixture = Fixture::new();
    fixture.write("migrations/0003_next.sql", b"SELECT 3;\n");
    fixture.commit();
    let destination = Fixture::empty();
    // Clone into a child so the independently owned fixture root remains the
    // cleanup boundary. --no-local forces the requested depth for local paths.
    let output = Command::new("git")
        .args(["clone", "--depth", "1", "--no-local"])
        .arg(&fixture.root)
        .arg(destination.root.join("shallow"))
        .output()
        .expect("Git should clone a local fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let error = check_at(&destination.root.join("shallow"), &fixture.base)
        .expect_err("missing published history cannot silently pass");
    assert!(error.to_string().contains("git rev-parse"));
}

#[cfg(unix)]
#[test]
fn published_symlink_is_rejected_without_reading_its_target() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    fs::remove_file(fixture.root.join(FIRST)).expect("fixture file can be removed");
    symlink("0002_extension.sql", fixture.root.join(FIRST))
        .expect("fixture symlink can be created");
    fixture.rejects("ordinary non-executable SQL file");
}

#[cfg(unix)]
#[test]
fn substituted_migration_directory_is_rejected() {
    let fixture = Fixture::new();
    fixture
        .check()
        .expect("ordinary migration directories pass");
    let original_bytes = fs::read(fixture.root.join(FIRST)).unwrap();
    fs::rename(
        fixture.root.join("migrations"),
        fixture.root.join("original"),
    )
    .expect("owned fixture directory can be renamed");
    let link = fixture.root.join("migrations");
    let target = fixture.root.join("original");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).expect("fixture symlink can be created");
    let _redirect = MigrationDirectoryLink(link);
    fixture.rejects("must be a real directory");
    assert_eq!(
        fs::read(target.join("0001_foundation.sql")).unwrap(),
        original_bytes,
        "rejecting the redirect preserves its target"
    );
}

#[cfg(unix)]
struct MigrationDirectoryLink(PathBuf);

#[cfg(unix)]
impl Drop for MigrationDirectoryLink {
    fn drop(&mut self) {
        #[cfg(unix)]
        fs::remove_file(&self.0).expect("remove only the owned migration symlink");
    }
}

#[test]
fn ambient_repository_and_index_redirects_are_neutralized() {
    let fixture = Fixture::new();
    let command = git::command(&fixture.root);
    let environment: std::collections::BTreeMap<_, _> = command.get_envs().collect();
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        assert_eq!(environment.get(std::ffi::OsStr::new(name)), Some(&None));
    }
    assert_eq!(
        environment.get(std::ffi::OsStr::new("GIT_CONFIG_NOSYSTEM")),
        Some(&Some(std::ffi::OsStr::new("1"))),
    );
}

#[test]
fn failed_git_diagnostics_do_not_echo_a_supplied_revision() {
    let fixture = Fixture::new();
    let revision = "private-sensitive-input-sentinel";
    let error = check_at(&fixture.root, revision).expect_err("unknown revisions fail");
    assert!(!error.to_string().contains(revision));
    assert!(error.to_string().contains("baseline availability"));
}

#[test]
fn git_metadata_and_blob_protocol_failures_do_not_pass() {
    assert!(git::parse_entries(b"garbage\0", false).is_err());
    assert!(git::parse_entries(b"100644 object 1\tmigrations/0001_one.sql\0", true).is_err());
    let entries = git::parse_entries(b"100644 blob object\tmigrations/0001_one.sql\0", false)
        .expect("synthetic metadata parses");
    for response in [
        b"object missing\n".as_slice(),
        b"object blob 999\nshort",
        b"object blob 1\na\nextra",
    ] {
        assert!(git::parse_blobs(&entries, response).is_err());
    }
}

struct Fixture {
    root: PathBuf,
    base: String,
}

impl Fixture {
    fn new() -> Self {
        let mut fixture = Self::empty();
        fixture.write(FIRST, b"SELECT 1;\n");
        fixture.write(SECOND, b"SELECT 2;\n");
        fixture.base = fixture.commit();
        fixture
    }

    fn empty() -> Self {
        let root = loop {
            let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir()
                .join(format!("fleetiq-migrations-{}-{id}", std::process::id()));
            match fs::create_dir(&root) {
                Ok(()) => break root,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("could not create migration fixture: {error}"),
            }
        };
        let fixture = Self {
            root,
            base: String::new(),
        };
        fixture.git(&["init", "--quiet"]);
        fixture
    }

    fn write(&self, path: &str, bytes: &[u8]) {
        let destination = self.root.join(path);
        fs::create_dir_all(destination.parent().expect("fixture path has a parent"))
            .expect("fixture directory can be created");
        fs::write(destination, bytes).expect("fixture content can be written");
    }

    fn commit(&self) -> String {
        self.git(&["add", "--all"]);
        self.git(&[
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "migration fixture",
        ]);
        String::from_utf8(self.git(&["rev-parse", "HEAD"]).stdout)
            .expect("commit ID is UTF-8")
            .trim()
            .to_owned()
    }

    fn git(&self, arguments: &[&str]) -> Output {
        let output = Command::new("git")
            .current_dir(&self.root)
            .args([
                "-c",
                "user.name=Migration sensor fixture",
                "-c",
                "user.email=migration-fixture@example.invalid",
                "-c",
                "core.autocrlf=false",
                "-c",
                "core.eol=lf",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(arguments)
            .output()
            .expect("Git is required for migration sensor tests");
        assert!(
            output.status.success(),
            "Git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn check(&self) -> Result<MigrationReport, MigrationCheckError> {
        check_at(&self.root, &self.base)
    }

    fn rejects(&self, expected: &str) {
        let error = self
            .check()
            .expect_err("invalid migration history must fail");
        assert!(
            error.to_string().contains(expected),
            "expected {expected}: {error}"
        );
        assert!(error.to_string().contains(GUIDE));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // The directory was created exclusively by this fixture, so cleanup
        // cannot recursively remove an existing repository or caller directory.
        let _ = fs::remove_dir_all(&self.root);
    }
}

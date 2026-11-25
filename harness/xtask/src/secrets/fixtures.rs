//! Real detector fixtures are separate from scanner orchestration and unit tests.

use std::{fs, path::Path};

use super::{
    SecretsError, check_with_detector, configuration,
    detector::Detector,
    failure, index,
    snapshot::{self, TemporaryDirectory, git},
};

pub(super) fn verify(root: &Path) -> Result<usize, SecretsError> {
    let detector = Detector::discover(root)?;
    let temporary = TemporaryDirectory::create()?;
    let fixture = temporary.path().join("repository");
    fs::create_dir(&fixture).map_err(|_| failure("could not create detector fixtures"))?;
    git(&fixture, &["init", "--quiet"])?;
    git(
        &fixture,
        &["config", "user.name", "FleetIQ harness fixture"],
    )?;
    git(
        &fixture,
        &["config", "user.email", "fixture@example.invalid"],
    )?;
    let policy = root.join(".gitleaks.toml");
    let token = synthetic_token();
    verify_suppression(&detector, &fixture, &policy, temporary.path(), &token)?;
    verify_exact_exceptions(&detector, &fixture, temporary.path(), &token)?;
    let base = verify_staged_history(&detector, &fixture, &policy, temporary.path(), &token)?;
    verify_trusted_policy(&detector, &fixture, temporary.path(), &token, &base)?;
    Ok(8)
}

/// Neither inline comments nor fingerprint files are valid suppression inputs.
fn verify_suppression(
    detector: &Detector,
    fixture: &Path,
    policy: &Path,
    temporary: &Path,
    token: &str,
) -> Result<(), SecretsError> {
    write(
        &fixture.join("candidate.txt"),
        &format!("token = \"{token}\" # gitleaks:allow\n"),
    )?;
    expect_findings(
        detector.scan_directory(fixture, policy, temporary),
        "inline suppression",
    )?;
    for (name, destination) in [
        (".gitleaksignore", "ignore-attempt"),
        (".GITLEAKSIGNORE", "case-ignore-attempt"),
    ] {
        let ignore = fixture.join(name);
        write(&ignore, "candidate.txt:github-pat:1\n")?;
        match snapshot::copy_candidates(fixture, &temporary.join(destination)) {
            Err(error)
                if error
                    .0
                    .starts_with("fingerprint ignore files are forbidden") => {}
            Err(error) => return Err(error),
            Ok(_) => {
                return Err(failure(
                    "fingerprint suppression fixture did not fail closed",
                ));
            }
        }
        fs::remove_file(ignore).map_err(|_| failure("could not reset the suppression fixture"))?;
    }
    Ok(())
}

/// A public fixture exception admits only one value in one file.
fn verify_exact_exceptions(
    detector: &Detector,
    fixture: &Path,
    temporary: &Path,
    token: &str,
) -> Result<(), SecretsError> {
    let allowlisted = temporary.join("exception.toml");
    let policy_source = exception_policy(token);
    configuration::validate(&policy_source)?;
    write(&allowlisted, &policy_source)?;
    detector.scan_directory(fixture, &allowlisted, temporary)?;
    let candidate = fixture.join("candidate.txt");
    write(&candidate, &format!("token = \"{}\"\n", alternate_token()))?;
    expect_findings(
        detector.scan_directory(fixture, &allowlisted, temporary),
        "new credential in an allowlisted file",
    )?;
    write(&candidate, "safe fixture\n")?;
    let other = fixture.join("other.txt");
    write(&other, &format!("token = \"{token}\"\n"))?;
    expect_findings(
        detector.scan_directory(fixture, &allowlisted, temporary),
        "allowlisted value in another file",
    )?;
    fs::remove_file(other).map_err(|_| failure("could not reset the exact path fixture"))?;
    Ok(())
}

/// Binary attributes, a restored worktree, and later removal cannot hide a leak.
fn verify_staged_history(
    detector: &Detector,
    fixture: &Path,
    policy: &Path,
    temporary: &Path,
    token: &str,
) -> Result<String, SecretsError> {
    let candidate = fixture.join("candidate.txt");
    write(
        &fixture.join(".gitleaks.toml"),
        "[extend]\nuseDefault = true\n",
    )?;
    write(&fixture.join(".gitattributes"), "candidate.txt binary\n")?;
    git(fixture, &["add", "."])?;
    git(fixture, &["commit", "--quiet", "-m", "safe base"])?;
    let base = snapshot::resolve_commit(fixture, "HEAD")?;
    write(&candidate, &format!("token = \"{token}\"\n"))?;
    git(fixture, &["add", "candidate.txt"])?;
    write(&candidate, "safe fixture\n")?;
    let staged = temporary.join("staged");
    index::copy_index(fixture, &staged)?;
    expect_findings(
        detector.scan_directory(&staged, policy, temporary),
        "raw staged credential with binary attributes",
    )?;
    git(
        fixture,
        &["commit", "--quiet", "-m", "synthetic positive fixture"],
    )?;
    git(fixture, &["add", "candidate.txt"])?;
    git(
        fixture,
        &[
            "commit",
            "--quiet",
            "-m",
            "remove synthetic positive fixture",
        ],
    )?;
    // The isolated bare copy cannot discover this working-tree ignore file.
    write(
        &fixture.join(".gitleaksignore"),
        "candidate.txt:github-pat:1\n",
    )?;
    let history = temporary.join("history.git");
    snapshot::copy_history(fixture, &history)?;
    expect_findings(
        detector.scan_history(&history, &format!("{base}..HEAD"), policy, temporary),
        "binary credential added then removed from history",
    )?;
    fs::remove_file(fixture.join(".gitleaksignore"))
        .map_err(|_| failure("could not reset the history fixture"))?;
    Ok(base)
}

/// Incoming policy may admit the value; trusted base policy must still reject it.
fn verify_trusted_policy(
    detector: &Detector,
    fixture: &Path,
    temporary: &Path,
    token: &str,
    base: &str,
) -> Result<(), SecretsError> {
    write(&fixture.join(".gitleaks.toml"), &exception_policy(token))?;
    git(fixture, &["add", ".gitleaks.toml"])?;
    git(
        fixture,
        &["commit", "--quiet", "-m", "incoming suppression attempt"],
    )?;
    let loose_history = temporary.join("loose-history.git");
    snapshot::copy_history(fixture, &loose_history)?;
    detector.scan_history(
        &loose_history,
        &format!("{base}..HEAD"),
        &temporary.join("exception.toml"),
        temporary,
    )?;
    expect_findings(
        check_with_detector(fixture, Some(base), detector).map(|_| ()),
        "incoming policy weakening",
    )
}

fn write(path: &Path, content: &str) -> Result<(), SecretsError> {
    fs::write(path, content).map_err(|_| failure("could not write a synthetic detector fixture"))
}

fn expect_findings(result: Result<(), SecretsError>, fixture: &str) -> Result<(), SecretsError> {
    match result {
        Err(error) if error.0.starts_with("secret findings:") => Ok(()),
        Err(error) => Err(error),
        Ok(()) => Err(failure(format!(
            "the pinned detector failed its {fixture} positive fixture"
        ))),
    }
}

fn exception_policy(token: &str) -> String {
    let token = token.replace("ghp_", r"ghp\_");
    format!(
        "[extend]\nuseDefault = true\n\n[[allowlists]]\ndescription = 'Owner: harness maintainers; Reason: synthetic detector fixture; Review trigger: detector upgrade'\ncondition = 'AND'\nregexTarget = 'secret'\npaths = ['^candidate\\.txt$']\nregexes = ['^{token}$']\n"
    )
}

pub(super) fn synthetic_token() -> String {
    ["ghp_", "wA9mK2pLxN4vRtQzY6bC8dEfGhJlM0oPq1rS"].concat()
}

fn alternate_token() -> String {
    ["ghp_", "qA9mK2pLxN4vRtQzY6bC8dEfGhJlM0oPq1rS"].concat()
}

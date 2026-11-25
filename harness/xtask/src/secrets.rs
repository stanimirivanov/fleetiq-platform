//! Repository-owned secret scanning with a pinned external detector.
//!
//! Candidate and raw index files are copied to isolated snapshots. Incoming
//! history is scanned from a bare local copy, preventing implicit fingerprint
//! ignores. Reports retain only paths, rule IDs, and line numbers.

mod configuration;
mod detector;
mod fixtures;
mod index;
mod snapshot;

#[cfg(test)]
mod tests;

use std::{error::Error, fmt, fs, path::Path};

use detector::Detector;
use snapshot::TemporaryDirectory;

pub(super) const VERSION: &str = "8.30.1";
const GUIDE: &str = "docs/development/harness-protection.md#secret-scanning";

/// Counts inputs verified without exposing detector report contents.
#[derive(Debug)]
pub(crate) struct SecretsReport {
    file_count: usize,
    history_checked: bool,
}

impl SecretsReport {
    pub(crate) const fn file_count(&self) -> usize {
        self.file_count
    }
    pub(crate) const fn history_checked(&self) -> bool {
        self.history_checked
    }
}

/// Carries only sanitized policy findings or operational failures.
#[derive(Debug)]
pub(crate) struct SecretsError(String);

impl fmt::Display for SecretsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}; see {GUIDE}", self.0)
    }
}
impl Error for SecretsError {}

pub(super) fn failure(message: impl Into<String>) -> SecretsError {
    SecretsError(message.into())
}

/// Scans current candidates, raw index blobs, and an explicit incoming range.
///
/// A supplied base must resolve to an ancestor commit. CI provides the trusted
/// event base, never a revision chosen by incoming content. Both current and
/// trusted base policies must pass, preventing incoming policy weakening. The
/// first policy addition is an explicitly reviewed bootstrap.
pub(crate) fn check(root: &Path, base: Option<&str>) -> Result<SecretsReport, SecretsError> {
    check_with_detector(root, base, &Detector::discover(root)?)
}

fn check_with_detector(
    root: &Path,
    base: Option<&str>,
    detector: &Detector,
) -> Result<SecretsReport, SecretsError> {
    let temporary = TemporaryDirectory::create()?;
    let current_files = temporary.path().join("files");
    let file_count = snapshot::copy_candidates(root, &current_files)?;
    let staged_files = temporary.path().join("index");
    index::copy_index(root, &staged_files)?;
    let current = fs::read_to_string(root.join(".gitleaks.toml"))
        .map_err(|_| failure("could not read the repository secret-scanning policy"))?;
    configuration::validate(&current)?;
    let current_path = temporary.path().join("current.toml");
    fs::write(&current_path, current)
        .map_err(|_| failure("could not prepare the isolated secret-scanning policy"))?;
    let mut policies = vec![current_path];
    let range = base
        .map(|value| snapshot::incoming_range(root, value))
        .transpose()?;
    if let Some((base_commit, _)) = &range
        && let Some(source) = snapshot::base_policy(root, base_commit)?
    {
        configuration::validate(&source)?;
        let base_path = temporary.path().join("base.toml");
        fs::write(&base_path, source)
            .map_err(|_| failure("could not prepare the trusted base secret-scanning policy"))?;
        policies.push(base_path);
    }
    let history = temporary.path().join("history.git");
    if range.is_some() {
        snapshot::copy_history(root, &history)?;
    }
    for policy in policies {
        detector.scan_directory(&current_files, &policy, temporary.path())?;
        detector.scan_directory(&staged_files, &policy, temporary.path())?;
        if let Some((_, range)) = &range {
            detector.scan_history(&history, range, &policy, temporary.path())?;
        }
    }
    Ok(SecretsReport {
        file_count,
        history_checked: range.is_some(),
    })
}

/// Verifies the real pinned detector and bypass-resistant input selection.
///
/// Missing tools and operational failures fail this command. Synthetic tokens
/// are assembled at runtime, and no matching source text is printed.
pub(crate) fn verify_fixtures(root: &Path) -> Result<usize, SecretsError> {
    fixtures::verify(root)
}

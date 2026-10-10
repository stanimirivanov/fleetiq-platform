//! Selects reviewable Git inputs and owns disposable snapshots.

use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

use super::{SecretsError, failure};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

pub(super) struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    pub(super) fn create() -> Result<Self, SecretsError> {
        for _ in 0..100 {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "fleetiq-secret-scan-{}-{sequence}",
                std::process::id()
            ));
            let builder = fs::DirBuilder::new();
            #[cfg(unix)]
            let builder = {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = builder;
                builder.mode(0o700);
                builder
            };
            match builder.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => {
                    return Err(failure(
                        "could not create the isolated secret-scanning directory",
                    ));
                }
            }
        }
        Err(failure(
            "could not allocate an isolated secret-scanning directory",
        ))
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        // No credentials are printed even when cleanup fails. A private temporary
        // directory remains recoverable by the host's normal temporary cleanup.
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn copy_candidates(root: &Path, destination: &Path) -> Result<usize, SecretsError> {
    fs::create_dir(destination).map_err(|_| failure("could not prepare the candidate snapshot"))?;
    let output = git(root, &["ls-files", "-co", "--exclude-standard", "-z"])?;
    let candidates = candidate_paths(&output.stdout)?;
    let mut copied = 0;
    for relative in candidates {
        reject_ignore(&relative)?;
        let source = root.join(&relative);
        reject_symlink_ancestors(root, &relative)?;
        let metadata = match fs::symlink_metadata(&source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(failure("could not inspect a Git candidate file")),
        };
        if metadata.file_type().is_symlink() {
            return Err(failure(
                "Git candidate symlinks are not accepted by secret scanning",
            ));
        }
        if !metadata.is_file() {
            return Err(failure(
                "Git candidate is not a regular file; submodules require an explicit scanning policy",
            ));
        }
        let target = destination.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|_| failure("could not prepare candidate snapshot directories"))?;
        }
        fs::copy(source, target)
            .map_err(|_| failure("could not copy a Git candidate into the scanning snapshot"))?;
        copied += 1;
    }
    Ok(copied)
}

pub(super) fn candidate_paths(bytes: &[u8]) -> Result<BTreeSet<PathBuf>, SecretsError> {
    let mut paths = BTreeSet::new();
    for value in bytes
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
    {
        let text = std::str::from_utf8(value)
            .map_err(|_| failure("Git candidate paths must be valid UTF-8"))?;
        let path = Path::new(text);
        if path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(failure(
                "Git candidate paths must remain inside the repository",
            ));
        }
        paths.insert(path.to_path_buf());
    }
    Ok(paths)
}

pub(super) fn incoming_range(root: &Path, base: &str) -> Result<(String, String), SecretsError> {
    let base = resolve_commit(root, base)?;
    let head = resolve_commit(root, "HEAD")?;
    git(root, &["merge-base", "--is-ancestor", &base, &head]).map_err(|_| {
        failure("secret-scanning base must be an ancestor commit; fetch the trusted event history")
    })?;
    Ok((base.clone(), format!("{base}..{head}")))
}

pub(super) fn resolve_commit(root: &Path, revision: &str) -> Result<String, SecretsError> {
    let expression = format!("{revision}^{{commit}}");
    let output = git(
        root,
        &["rev-parse", "--verify", "--end-of-options", &expression],
    )?;
    let value = std::str::from_utf8(&output.stdout)
        .map_err(|_| failure("Git returned an invalid commit identifier"))?
        .trim();
    if !matches!(value.len(), 40 | 64) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(failure("Git returned an invalid commit identifier"));
    }
    Ok(value.to_owned())
}

pub(super) fn base_policy(root: &Path, base: &str) -> Result<Option<String>, SecretsError> {
    let listing = git(
        root,
        &["ls-tree", "--name-only", base, "--", ".gitleaks.toml"],
    )?;
    if listing.stdout.is_empty() {
        return Ok(None);
    }
    let reference = format!("{base}:.gitleaks.toml");
    let output = git(root, &["show", &reference])?;
    String::from_utf8(output.stdout)
        .map(Some)
        .map_err(|_| failure("trusted base secret-scanning policy must be UTF-8"))
}

pub(super) fn git(root: &Path, arguments: &[&str]) -> Result<Output, SecretsError> {
    let mut command = Command::new("git");
    let canonical = trusted_root(root)?;
    command
        .arg("-c")
        .arg(format!("safe.directory={canonical}"))
        .current_dir(root)
        .args(arguments);
    sanitize_environment(&mut command);
    let output = command
        .output()
        .map_err(|_| failure("Git is unavailable for secret-scanning input selection"))?;
    if !output.status.success() {
        return Err(failure(
            "Git input selection failed; verify repository history and the trusted base",
        ));
    }
    Ok(output)
}

pub(super) fn sanitize_environment(command: &mut Command) {
    for (name, _) in std::env::vars_os() {
        let text = name.to_string_lossy().to_ascii_uppercase();
        if text.starts_with("GITLEAKS_")
            || text.starts_with("GIT_CONFIG_")
            || matches!(
                text.as_str(),
                "GIT_EXTERNAL_DIFF"
                    | "GIT_DIFF_OPTS"
                    | "GIT_DIR"
                    | "GIT_WORK_TREE"
                    | "GIT_INDEX_FILE"
                    | "GIT_COMMON_DIR"
                    | "GIT_NAMESPACE"
                    | "GIT_OBJECT_DIRECTORY"
                    | "GIT_ALTERNATE_OBJECT_DIRECTORIES"
            )
        {
            command.env_remove(name);
        }
    }
    command.env("GIT_CONFIG_NOSYSTEM", "1").env(
        "GIT_CONFIG_GLOBAL",
        if cfg!(windows) { "NUL" } else { "/dev/null" },
    );
}

pub(super) fn reject_ignore(relative: &Path) -> Result<(), SecretsError> {
    if relative.file_name().is_some_and(|name| {
        name.to_string_lossy()
            .eq_ignore_ascii_case(".gitleaksignore")
    }) {
        return Err(failure(
            "fingerprint ignore files are forbidden; use reviewed exact nonproduction policy exceptions",
        ));
    }
    Ok(())
}

/// Inspect every component before reading bytes. A regular final file does not
/// prove that a replaced ancestor directory stays inside the repository.
pub(super) fn reject_symlink_ancestors(root: &Path, relative: &Path) -> Result<(), SecretsError> {
    let canonical_root = root
        .canonicalize()
        .map_err(|_| failure("could not resolve the repository root"))?;
    let mut source = root.to_path_buf();
    for component in relative.components() {
        source.push(component.as_os_str());
        match fs::symlink_metadata(&source) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(failure(
                    "Git candidate symlinks and replaced directory links are not accepted by secret scanning",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(failure("could not inspect Git candidate ancestors")),
        }
    }
    let canonical = source
        .canonicalize()
        .map_err(|_| failure("could not resolve a Git candidate"))?;
    if !canonical.starts_with(canonical_root) {
        return Err(failure("Git candidate resolved outside the repository"));
    }
    Ok(())
}

/// A bare local copy prevents upstream implicit working-tree ignore discovery.
/// Only local Git objects are copied; no remote is contacted and no hooks run.
pub(super) fn copy_history(root: &Path, destination: &Path) -> Result<(), SecretsError> {
    let source = root
        .to_str()
        .ok_or_else(|| failure("repository path must be UTF-8"))?;
    let destination = destination
        .to_str()
        .ok_or_else(|| failure("snapshot path must be UTF-8"))?;
    git(
        root,
        &[
            "clone",
            "--bare",
            "--local",
            "--no-hardlinks",
            "--quiet",
            "--",
            source,
            destination,
        ],
    )?;
    Ok(())
}

pub(super) fn trusted_root(root: &Path) -> Result<String, SecretsError> {
    let canonical = root
        .canonicalize()
        .map_err(|_| failure("could not resolve the authorized Git root"))?;
    let display = canonical
        .to_str()
        .ok_or_else(|| failure("authorized Git root must be UTF-8"))?;
    if cfg!(windows) {
        let normalized = display.replace(std::path::MAIN_SEPARATOR, "/");
        Ok(normalized
            .strip_prefix("//?/")
            .unwrap_or(&normalized)
            .to_owned())
    } else {
        Ok(display.to_owned())
    }
}

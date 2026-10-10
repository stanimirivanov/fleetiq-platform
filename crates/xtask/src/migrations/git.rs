//! Reads Git evidence without shell interpolation or SQL content in diagnostics.

use std::{
    collections::BTreeMap,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
    thread,
};

use super::MigrationCheckError;

#[derive(Debug)]
pub(super) struct Entry {
    pub(super) mode: String,
    pub(super) object: String,
}

pub(super) fn resolve_base(root: &Path, base: &str) -> Result<String, MigrationCheckError> {
    let object = format!("{base}^{{commit}}");
    let output = run(
        root,
        &["rev-parse", "--verify", "--end-of-options", &object],
    )?;
    let commit = text(&output.stdout, "resolve baseline commit")?
        .trim()
        .to_owned();
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(MigrationCheckError::invalid(
            "Git returned an invalid baseline commit identifier; verification cannot continue",
        ));
    }
    let ancestry = command(root)
        .args(["merge-base", "--is-ancestor", &commit, "HEAD"])
        .output()
        .map_err(|source| MigrationCheckError::io("check baseline ancestry", source))?;
    match ancestry.status.code() {
        Some(0) => Ok(commit),
        Some(1) => Err(MigrationCheckError::invalid(
            "the baseline is not an ancestor of HEAD; fetch the trusted published history and select its actual baseline commit",
        )),
        _ => Err(failed("check baseline ancestry", &ancestry)),
    }
}

pub(super) fn published_entries(
    root: &Path,
    commit: &str,
) -> Result<BTreeMap<String, Entry>, MigrationCheckError> {
    let output = run(root, &["ls-tree", "-r", "-z", commit, "--", "migrations"])?;
    parse_entries(&output.stdout, false)
}

pub(super) fn index_entries(root: &Path) -> Result<BTreeMap<String, Entry>, MigrationCheckError> {
    let output = run(root, &["ls-files", "--stage", "-z", "--", "migrations"])?;
    parse_entries(&output.stdout, true)
}

pub(super) fn parse_entries(
    bytes: &[u8],
    index: bool,
) -> Result<BTreeMap<String, Entry>, MigrationCheckError> {
    let mut entries = BTreeMap::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|item| !item.is_empty())
    {
        let record = text(record, "read migration paths")?;
        let (header, path) = record.split_once('\t').ok_or_else(|| {
            MigrationCheckError::invalid("Git returned a malformed migration entry")
        })?;
        let fields: Vec<_> = header.split_whitespace().collect();
        if fields.len() != 3 || (index && fields[2] != "0") || (!index && fields[1] != "blob") {
            return Err(MigrationCheckError::invalid(format!(
                "`{path}` is not a resolved regular migration blob; resolve merge conflicts and use a regular SQL file",
            )));
        }
        let object = fields[if index { 1 } else { 2 }].to_owned();
        if entries
            .insert(
                path.to_owned(),
                Entry {
                    mode: fields[0].to_owned(),
                    object,
                },
            )
            .is_some()
        {
            return Err(MigrationCheckError::invalid(format!(
                "Git returned multiple entries for `{path}`; resolve the index before verification",
            )));
        }
    }
    Ok(entries)
}

/// Batches blob reads and concurrently feeds stdin to avoid pipe-buffer deadlock
/// if the migration chain eventually grows beyond the platform's pipe capacity.
pub(super) fn read_blobs(
    root: &Path,
    entries: &BTreeMap<String, Entry>,
) -> Result<BTreeMap<String, Vec<u8>>, MigrationCheckError> {
    let mut requests = String::new();
    for entry in entries.values() {
        requests.push_str(&entry.object);
        requests.push('\n');
    }
    let mut child = command(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| MigrationCheckError::io("read baseline migration blobs", source))?;
    let mut input = child.stdin.take().ok_or_else(|| {
        MigrationCheckError::invalid("Git blob reader did not provide the requested input pipe")
    })?;
    let output = thread::scope(|scope| {
        let writer = scope.spawn(move || input.write_all(requests.as_bytes()));
        let output = child.wait_with_output();
        let result = writer.join().map_err(|_| {
            MigrationCheckError::invalid("Git blob request writer failed; rerun verification")
        })?;
        result.map_err(|source| MigrationCheckError::io("write Git blob requests", source))?;
        output.map_err(|source| MigrationCheckError::io("read baseline migration blobs", source))
    })?;
    if !output.status.success() {
        return Err(failed("read baseline migration blobs", &output));
    }
    parse_blobs(entries, &output.stdout)
}

pub(super) fn parse_blobs(
    entries: &BTreeMap<String, Entry>,
    mut bytes: &[u8],
) -> Result<BTreeMap<String, Vec<u8>>, MigrationCheckError> {
    let malformed = || {
        MigrationCheckError::invalid(
            "Git returned incomplete baseline blob evidence; fetch the required history and rerun verification",
        )
    };
    let mut blobs = BTreeMap::new();
    for (path, entry) in entries {
        let header_end = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or_else(malformed)?;
        let header = text(&bytes[..header_end], "read Git blob header")?;
        let fields: Vec<_> = header.split_whitespace().collect();
        if fields.len() != 3 || fields[0] != entry.object || fields[1] != "blob" {
            return Err(malformed());
        }
        let size = fields[2].parse::<usize>().map_err(|_| malformed())?;
        let body = &bytes[header_end + 1..];
        if body.get(size) != Some(&b'\n') {
            return Err(malformed());
        }
        blobs.insert(path.clone(), body[..size].to_vec());
        bytes = &body[size + 1..];
    }
    if !bytes.is_empty() {
        return Err(malformed());
    }
    Ok(blobs)
}

pub(super) fn command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(root);
    // Trust only the caller-selected checkout when a sandbox or container user
    // differs from its owner; never inherit a wildcard ownership exception.
    command.args(["-c", &format!("safe.directory={}", trusted_directory(root))]);
    // Verification concerns this checkout and its real index. An ambient
    // alternate index or Git directory must not conceal staged changes.
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(name);
    }
    for (name, _) in std::env::vars_os() {
        if name
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("GIT_CONFIG_")
        {
            command.env_remove(name);
        }
    }
    command.env("GIT_CONFIG_NOSYSTEM", "1").env(
        "GIT_CONFIG_GLOBAL",
        if cfg!(windows) { "NUL" } else { "/dev/null" },
    );
    command
}

fn trusted_directory(root: &Path) -> String {
    let directory = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let display = directory.to_string_lossy();
    #[cfg(windows)]
    {
        // Windows canonical paths use the extended-length prefix. Git's safe
        // directory comparison expects an ordinary drive or UNC path.
        let normalized = display.replace(std::path::MAIN_SEPARATOR, "/");
        if let Some(unc) = normalized.strip_prefix("//?/UNC/") {
            format!("//{unc}")
        } else {
            normalized
                .strip_prefix("//?/")
                .unwrap_or(&normalized)
                .to_owned()
        }
    }
    #[cfg(not(windows))]
    {
        display.into_owned()
    }
}

fn run(root: &Path, arguments: &[&str]) -> Result<Output, MigrationCheckError> {
    let operation = format!("git {} for migration verification", arguments[0]);
    let output = command(root)
        .args(arguments)
        .output()
        .map_err(|source| MigrationCheckError::io(&operation, source))?;
    if !output.status.success() {
        return Err(failed(&operation, &output));
    }
    Ok(output)
}

fn failed(operation: &str, output: &Output) -> MigrationCheckError {
    MigrationCheckError::Git {
        operation: operation.to_owned(),
        // Git stderr can echo an invalid revision or local configuration value.
        // Retain status and recovery guidance without exposing those inputs.
        detail: format!(
            "Git exited with {}; verify repository history, baseline availability, and local Git configuration",
            output.status,
        ),
    }
}

fn text<'a>(bytes: &'a [u8], operation: &str) -> Result<&'a str, MigrationCheckError> {
    std::str::from_utf8(bytes).map_err(|_| {
        MigrationCheckError::invalid(format!(
            "{operation}: non-UTF-8 Git metadata is not supported"
        ))
    })
}

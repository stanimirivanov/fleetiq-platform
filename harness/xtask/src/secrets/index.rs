//! Reads raw staged blobs without Git diff attributes, filters, or textconv.

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
};

use super::{
    SecretsError, failure,
    snapshot::{candidate_paths, git, reject_ignore, sanitize_environment, trusted_root},
};

pub(super) fn copy_index(root: &Path, destination: &Path) -> Result<(), SecretsError> {
    fs::create_dir(destination).map_err(|_| failure("could not prepare the staged snapshot"))?;
    let output = git(root, &["ls-files", "--stage", "-z"])?;
    let entries = parse_entries(&output.stdout)?;
    let blobs = read_blobs(root, &entries)?;
    for (path, bytes) in blobs {
        let target = destination.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|_| failure("could not prepare staged snapshot directories"))?;
        }
        fs::write(target, bytes).map_err(|_| failure("could not write the raw staged snapshot"))?;
    }
    Ok(())
}

pub(super) fn parse_entries(bytes: &[u8]) -> Result<BTreeMap<PathBuf, String>, SecretsError> {
    let mut entries = BTreeMap::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|item| !item.is_empty())
    {
        let text =
            std::str::from_utf8(record).map_err(|_| failure("Git index metadata must be UTF-8"))?;
        let (header, path) = text
            .split_once('\t')
            .ok_or_else(|| failure("Git returned a malformed index entry"))?;
        let fields: Vec<_> = header.split_whitespace().collect();
        if fields.len() != 3 || fields[2] != "0" || !matches!(fields[0], "100644" | "100755") {
            return Err(failure(
                "secret scanning requires resolved regular index files; symlinks and submodules are not admitted",
            ));
        }
        if !matches!(fields[1].len(), 40 | 64)
            || !fields[1].bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(failure("Git returned an invalid index object identifier"));
        }
        let selected = candidate_paths(path.as_bytes())?;
        let path = selected
            .into_iter()
            .next()
            .ok_or_else(|| failure("Git returned an empty index path"))?;
        reject_ignore(&path)?;
        if entries.insert(path, fields[1].to_owned()).is_some() {
            return Err(failure("Git index entries must be unique and resolved"));
        }
    }
    Ok(entries)
}

fn read_blobs(
    root: &Path,
    entries: &BTreeMap<PathBuf, String>,
) -> Result<BTreeMap<PathBuf, Vec<u8>>, SecretsError> {
    let mut requests = String::new();
    for object in entries.values() {
        requests.push_str(object);
        requests.push('\n');
    }
    let mut command = Command::new("git");
    let canonical = trusted_root(root)?;
    command
        .arg("-c")
        .arg(format!("safe.directory={canonical}"))
        .current_dir(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    sanitize_environment(&mut command);
    let mut child = command
        .spawn()
        .map_err(|_| failure("could not read raw index blobs"))?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| failure("Git did not provide a blob request pipe"))?;
    let output = thread::scope(|scope| {
        let writer = scope.spawn(move || input.write_all(requests.as_bytes()));
        let output = child.wait_with_output();
        writer
            .join()
            .map_err(|_| failure("Git index request writer failed"))?
            .map_err(|_| failure("could not submit raw index blob requests"))?;
        output.map_err(|_| failure("could not receive raw index blob evidence"))
    })?;
    if !output.status.success() {
        return Err(failure("Git raw index blob reader failed"));
    }
    parse_blobs(entries, &output.stdout)
}

pub(super) fn parse_blobs(
    entries: &BTreeMap<PathBuf, String>,
    mut bytes: &[u8],
) -> Result<BTreeMap<PathBuf, Vec<u8>>, SecretsError> {
    let malformed = || failure("Git returned incomplete raw index blob evidence");
    let mut blobs = BTreeMap::new();
    for (path, object) in entries {
        let end = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or_else(malformed)?;
        let header = std::str::from_utf8(&bytes[..end]).map_err(|_| malformed())?;
        let fields: Vec<_> = header.split_whitespace().collect();
        if fields.len() != 3 || fields[0] != object || fields[1] != "blob" {
            return Err(malformed());
        }
        let size = fields[2].parse::<usize>().map_err(|_| malformed())?;
        let body = &bytes[end + 1..];
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

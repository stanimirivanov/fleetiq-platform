//! Owns SQL filename, version, file-kind, and byte-immutability policy.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use super::{MigrationCheckError, git::Entry};

pub(super) fn verify(
    root: &Path,
    published: &BTreeMap<String, Entry>,
    index: &BTreeMap<String, Entry>,
    blobs: &BTreeMap<String, Vec<u8>>,
) -> Result<usize, MigrationCheckError> {
    let mut violations = Vec::new();
    let baseline_versions = versions(published.keys().map(String::as_str), &mut violations);
    let highest = baseline_versions.keys().next_back().copied().unwrap_or(0);
    verify_index(published, index, highest, &mut violations);
    let current = current_files(root, &mut violations)?;
    versions(current.iter().map(String::as_str), &mut violations);
    let mut added_count = 0;
    for path in &current {
        if !published.contains_key(path) {
            added_count += 1;
            if let Ok(version) = version(path)
                && version <= highest
            {
                violations.push(format!(
                    "new migration `{path}` uses version {version}, at or below published high-water mark {highest}; add a forward migration with a newer version",
                ));
            }
        }
    }
    for (path, entry) in published {
        if entry.mode != "100644" {
            violations.push(format!(
                "published migration `{path}` has unsupported Git mode {}; baseline migrations must be ordinary non-executable files",
                entry.mode,
            ));
        }
        if !current.contains(path) {
            violations.push(format!(
                "published migration `{path}` is missing; restore its original path and add a forward migration instead of deleting or renaming history",
            ));
            continue;
        }
        let bytes = fs::read(root.join(path)).map_err(|source| {
            MigrationCheckError::io(format!("read migration `{path}`"), source)
        })?;
        if blobs.get(path) != Some(&bytes) {
            violations.push(format!(
                "published migration `{path}` has changed bytes; restore the published file exactly, including comments and line endings, and add a forward migration",
            ));
        }
    }
    for path in index.keys() {
        if !current.contains(path) && !published.contains_key(path) {
            violations.push(format!(
                "staged migration `{path}` is missing from the working tree; restore or unstage it before verification",
            ));
        }
    }
    violations.sort();
    violations.dedup();
    if violations.is_empty() {
        Ok(added_count)
    } else {
        Err(MigrationCheckError::Policy(violations))
    }
}

fn verify_index(
    published: &BTreeMap<String, Entry>,
    index: &BTreeMap<String, Entry>,
    highest: i64,
    violations: &mut Vec<String>,
) {
    versions(index.keys().map(String::as_str), violations);
    for (path, entry) in index {
        if entry.mode != "100644" {
            violations.push(format!(
                "staged migration `{path}` has Git mode {}; use an ordinary non-executable SQL file, never a symlink or executable",
                entry.mode,
            ));
        }
        if let Some(original) = published.get(path) {
            if entry.object != original.object || entry.mode != original.mode {
                violations.push(format!(
                    "published migration `{path}` has a staged content or mode change; restore the original index entry and use a forward migration",
                ));
            }
        } else if let Ok(version) = version(path)
            && version <= highest
        {
            violations.push(format!(
                "staged new migration `{path}` reuses published version range through {highest}; add a newer forward migration",
            ));
        }
    }
    for path in published.keys().filter(|path| !index.contains_key(*path)) {
        violations.push(format!(
            "published migration `{path}` is absent from the Git index; restore its tracked entry instead of deleting or renaming history",
        ));
    }
}

fn current_files(
    root: &Path,
    violations: &mut Vec<String>,
) -> Result<BTreeSet<String>, MigrationCheckError> {
    let directory = root.join("migrations");
    let metadata = fs::symlink_metadata(&directory)
        .map_err(|source| MigrationCheckError::io("read migrations directory", source))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(MigrationCheckError::invalid(
            "`migrations` must be a real directory, not a symlink or substituted file; restore the migration chain",
        ));
    }
    let mut paths = BTreeSet::new();
    for entry in fs::read_dir(&directory)
        .map_err(|source| MigrationCheckError::io("list working-tree migrations", source))?
    {
        let entry =
            entry.map_err(|source| MigrationCheckError::io("read migration entry", source))?;
        let name = entry.file_name().into_string().map_err(|_| {
            MigrationCheckError::invalid("migration filenames must use ASCII numeric versions and lowercase snake_case names")
        })?;
        let path = format!("migrations/{name}");
        let metadata = fs::symlink_metadata(entry.path()).map_err(|source| {
            MigrationCheckError::io(format!("inspect migration `{path}`"), source)
        })?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || executable(&metadata) {
            violations.push(format!(
                "migration `{path}` must be an ordinary non-executable SQL file; remove nested directories, symlinks, and executable modes",
            ));
            continue;
        }
        paths.insert(path);
    }
    Ok(paths)
}

#[cfg(unix)]
fn executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_metadata: &fs::Metadata) -> bool {
    // The Git index carries the
    // tracked mode and is checked separately on every platform.
    false
}

fn versions<'a>(
    paths: impl Iterator<Item = &'a str>,
    violations: &mut Vec<String>,
) -> BTreeMap<i64, &'a str> {
    let mut versions = BTreeMap::new();
    for path in paths {
        match version(path) {
            Ok(number) => {
                if let Some(previous) = versions.insert(number, path) {
                    violations.push(format!(
                        "migrations `{previous}` and `{path}` share numeric version {number}; give each forward migration a unique newer version",
                    ));
                }
            }
            Err(detail) => violations.push(format!("invalid migration `{path}`: {detail}")),
        }
    }
    versions
}

fn version(path: &str) -> Result<i64, &'static str> {
    const FORMAT: &str = "use <version>_<lowercase_snake_case>.sql with at least four version digits and a positive signed-64-bit version";
    let name = path.strip_prefix("migrations/").ok_or(FORMAT)?;
    let stem = name.strip_suffix(".sql").ok_or(FORMAT)?;
    let (digits, description) = stem.split_once('_').ok_or(FORMAT)?;
    if digits.len() < 4
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
        || description.is_empty()
        || description.split('_').any(|word| {
            word.is_empty()
                || !word
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
        || !description.as_bytes()[0].is_ascii_lowercase()
    {
        return Err(FORMAT);
    }
    digits
        .parse::<i64>()
        .ok()
        .filter(|number| *number > 0)
        .ok_or(FORMAT)
}

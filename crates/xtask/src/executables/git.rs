use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::Path,
};

use crate::migrations::git::command;

use super::{
    GUIDE,
    surfaces::{self, Surface, Target},
};

pub(super) struct Snapshot<'a> {
    root: &'a Path,
    source: Source<'a>,
    entries: BTreeMap<String, String>,
}

#[derive(Clone, Copy)]
enum Source<'a> {
    Tree(&'a str),
    Index,
    Worktree,
}

impl<'a> Snapshot<'a> {
    pub(super) fn tree(root: &'a Path, reference: &'a str) -> Result<Self, String> {
        Self::git_entries(
            root,
            Source::Tree(reference),
            &["ls-tree", "-r", "-z", reference],
        )
    }

    pub(super) fn index(root: &'a Path) -> Result<Self, String> {
        Self::git_entries(root, Source::Index, &["ls-files", "--stage", "-z"])
    }

    pub(super) fn worktree(root: &'a Path) -> Result<Self, String> {
        let output = output(
            root,
            &[
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "-z",
            ],
        )?;
        let mut entries = BTreeMap::new();
        let canonical_root = root
            .canonicalize()
            .map_err(|_| failed("could not resolve the working tree"))?;
        for path in records(&output)? {
            let metadata = match fs::symlink_metadata(root.join(path)) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(failed("could not inspect a working tree entry")),
            };
            if metadata.is_symlink() || metadata.is_file() || metadata.is_dir() {
                let mode = if metadata.is_symlink() {
                    "120000"
                } else if metadata.is_dir() {
                    // A checkout may replace a tracked file with a directory. Its
                    // reviewable children are already in the worktree enumeration.
                    continue;
                } else {
                    // A regular leaf can still be reached through a replaced
                    // directory link. Reject it before opening source bytes.
                    if root.join(path).canonicalize().ok().as_ref()
                        != Some(&canonical_root.join(path))
                    {
                        return Err(failed(
                            "a working tree file has a linked or unreadable ancestor",
                        ));
                    }
                    if executable(&metadata) {
                        "100755"
                    } else {
                        "100644"
                    }
                };
                entries.insert(path.to_owned(), mode.to_owned());
            }
        }
        Ok(Self {
            root,
            source: Source::Worktree,
            entries,
        })
    }

    fn git_entries(root: &'a Path, source: Source<'a>, args: &[&str]) -> Result<Self, String> {
        let bytes = output(root, args)?;
        let mut entries = BTreeMap::new();
        for record in records(&bytes)? {
            let Some((header, path)) = record.split_once('\t') else {
                return Err(failed("could not parse a Git entry"));
            };
            let fields: Vec<_> = header.split_ascii_whitespace().collect();
            if fields.len() != 3 || matches!(source, Source::Index) && fields[2] != "0" {
                return Err(failed("could not inspect an unresolved Git entry"));
            }
            if entries
                .insert(path.to_owned(), fields[0].to_owned())
                .is_some()
            {
                return Err(failed("Git reported a duplicate path"));
            }
        }
        Ok(Self {
            root,
            source,
            entries,
        })
    }

    pub(super) fn contains(&self, path: &str) -> bool {
        self.entries.contains_key(path)
    }

    pub(super) fn read(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        if !self.contains(path) {
            return Ok(None);
        }
        if !matches!(self.entries[path].as_str(), "100644" | "100755") {
            return Err(failed("a configuration path is not a regular file"));
        }
        let bytes = match self.source {
            Source::Tree(reference) => output(
                self.root,
                &["cat-file", "blob", &format!("{reference}:{path}")],
            )?,
            Source::Index => output(self.root, &["cat-file", "blob", &format!(":{path}")])?,
            Source::Worktree => fs::read(self.root.join(path))
                .map_err(|_| failed("could not read a working tree entry"))?,
        };
        Ok(Some(bytes))
    }

    pub(super) fn surfaces(&self) -> Result<BTreeSet<Surface>, String> {
        let mut found = BTreeSet::new();
        let shebangs = self.shebang_candidates()?;
        for (path, mode) in &self.entries {
            let nonregular = matches!(mode.as_str(), "120000" | "160000");
            if nonregular
                || mode == "100755"
                || surfaces::is_script(path)
                || self.has_shebang(path, &shebangs)?
            {
                found.insert(Surface::new(path, Target::Script));
            }
            if surfaces::is_configuration(path) {
                let source = self
                    .read(path)?
                    .ok_or_else(|| failed("configuration disappeared"))?;
                surfaces::configuration(&mut found, path, &source)?;
            }
        }
        Ok(found)
    }

    fn has_shebang(&self, path: &str, candidates: &BTreeSet<String>) -> Result<bool, String> {
        if matches!(self.source, Source::Worktree) {
            let mut prefix = Vec::new();
            File::open(self.root.join(path))
                .and_then(|file| file.take(256).read_to_end(&mut prefix))
                .map_err(|_| failed("could not read a working tree file prefix"))?;
            return Ok(surfaces::has_shebang(&prefix));
        }
        if candidates.contains(path) {
            let source = self
                .read(path)?
                .ok_or_else(|| failed("Git candidate disappeared"))?;
            return Ok(surfaces::has_shebang(&source));
        }
        Ok(false)
    }

    fn shebang_candidates(&self) -> Result<BTreeSet<String>, String> {
        let mut args = vec!["grep", "--full-name", "-l", "-z", "-a", "-e", "^#!"];
        match self.source {
            Source::Tree(reference) => args.push(reference),
            Source::Index => args.push("--cached"),
            Source::Worktree => return Ok(BTreeSet::new()),
        }
        let result = command(self.root)
            .args(args)
            .output()
            .map_err(|_| failed("could not query Git scripts"))?;
        match result.status.code() {
            Some(0) => records(&result.stdout)?
                .map(|entry| match self.source {
                    Source::Tree(reference) => entry
                        .strip_prefix(reference)
                        .and_then(|path| path.strip_prefix(':'))
                        .map(str::to_owned)
                        .ok_or_else(|| failed("could not parse a Git tree grep path")),
                    _ => Ok(entry.to_owned()),
                })
                .collect(),
            Some(1) => Ok(BTreeSet::new()),
            _ => Err(failed("could not query Git scripts")),
        }
    }
}

#[cfg(unix)]
fn executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    // Git stores the owner execute bit as the tracked file mode.
    metadata.permissions().mode() & 0o100 != 0
}

#[cfg(not(unix))]
fn executable(_: &fs::Metadata) -> bool {
    false // The Git index retains portable executable-bit information.
}

fn records(bytes: &[u8]) -> Result<impl Iterator<Item = &str>, String> {
    Ok(std::str::from_utf8(bytes)
        .map_err(|_| failed("Git returned a non-UTF-8 path"))?
        .split_terminator('\0'))
}

fn output(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let result = command(root)
        .args(args)
        .output()
        .map_err(|_| failed("could not read Git information"))?;
    if !result.status.success() {
        return Err(failed("could not read Git information"));
    }
    Ok(result.stdout)
}

fn failed(why: &str) -> String {
    format!("EXEC_REGISTRY FAIL {why}\nFix: restore a readable checkout and index\nRead: {GUIDE}")
}

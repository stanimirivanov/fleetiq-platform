mod document;
mod links;

#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use super::{DOCUMENTATION_POLICY, Repository, Violation};
use document::parse;

pub(super) fn parse_fragment(relative_path: &str, body: &str) -> Document {
    parse(relative_path.to_owned(), body.to_owned())
}

const EXCLUDED_DIRECTORIES: &[&str] = &[
    ".git",
    ".idea",
    ".local",
    ".ruff_cache",
    ".venv",
    "bin",
    "node_modules",
    "target",
    "testdata",
    "vendor",
];
const REPOSITORY_POLICY: &str = "docs/development/harness.md#repository-policy";

#[derive(Debug)]
pub(crate) struct Document {
    relative_path: String,
    body: String,
    headings: Vec<Heading>,
    links: Vec<MarkdownLink>,
    code_blocks: Vec<MarkdownCodeBlock>,
    table_rows: Vec<MarkdownTableRow>,
    word_count: usize,
    visible_lines: BTreeMap<usize, String>,
    raw_html_lines: std::collections::BTreeSet<usize>,
}

impl Document {
    pub(super) fn body(&self) -> &str {
        &self.body
    }

    pub(super) fn headings(&self) -> &[Heading] {
        &self.headings
    }

    pub(super) fn line_is_visible(&self, line: usize) -> bool {
        self.visible_lines.contains_key(&line)
    }

    pub(super) fn visible_line_text(&self, line: usize) -> Option<&str> {
        self.visible_lines.get(&line).map(String::as_str)
    }

    #[cfg(test)]
    pub(super) fn visible_text_contains(&self, marker: &str) -> bool {
        self.visible_lines
            .values()
            .any(|line| line.contains(marker))
    }

    pub(super) fn code_blocks(&self) -> &[MarkdownCodeBlock] {
        &self.code_blocks
    }

    pub(super) fn table_rows(&self) -> &[MarkdownTableRow] {
        &self.table_rows
    }

    pub(super) fn first_raw_html_between(&self, start: usize, end: usize) -> Option<usize> {
        self.raw_html_lines.range(start..end).next().copied()
    }

    #[cfg(test)]
    pub(super) fn contains_raw_html(&self) -> bool {
        !self.raw_html_lines.is_empty()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Heading {
    pub(super) level: u8,
    pub(super) line: usize,
    pub(super) text: String,
    pub(super) anchor: String,
}

#[derive(Debug, Eq, PartialEq)]
struct MarkdownLink {
    line: usize,
    destination: String,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct MarkdownCodeBlock {
    pub(super) line: usize,
    pub(super) info: String,
    pub(super) body: String,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct MarkdownTableRow {
    pub(super) line: usize,
    pub(super) cells: Vec<String>,
}

/// Loads every policy-controlled Markdown document below `root`.
pub(super) fn load(root: &Path) -> Result<Repository, String> {
    let mut paths = Vec::new();
    discover(root, root, &mut paths).map_err(|error| {
        format!(
            "could not discover Markdown documents below {}: {error}",
            root.display()
        )
    })?;
    paths.sort();

    let mut documents = BTreeMap::new();
    for path in paths {
        let body = fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        let relative_path = relative_path(root, &path)?;
        let document = parse(relative_path.clone(), body);
        documents.insert(relative_path, document);
    }

    Ok(Repository {
        root: root.to_path_buf(),
        documents,
    })
}

/// Checks local Markdown destinations, exact path casing, and heading anchors.
pub(super) fn check_links(repository: &Repository) -> Vec<Violation> {
    repository
        .documents
        .values()
        .flat_map(|document| links::validate(repository, document))
        .collect()
}

/// Enforces the contributor guide's summary requirement for long-form guides.
pub(super) fn check_tldr(repository: &Repository) -> Vec<Violation> {
    repository
        .documents
        .values()
        .filter(|document| requires_tldr(document))
        .filter_map(|document| {
            let title = document
                .headings
                .iter()
                .position(|heading| heading.level == 1);
            let first_h2 = document.headings.iter().find(|heading| heading.level == 2);
            let summary = document
                .headings
                .iter()
                .position(|heading| heading.level == 2);
            if first_h2.is_some_and(|heading| {
                heading.text.trim() == "TL;DR"
                    && document
                        .first_raw_html_between(heading.line, heading.line + 1)
                        .is_none()
            }) && title.is_some_and(|title| summary.is_some_and(|summary| title < summary))
                && title.is_some_and(|title| {
                    first_h2.is_some_and(|summary| {
                        no_prose_before_summary(
                            document,
                            document.headings[title].line,
                            summary.line,
                        )
                    })
                })
                && summary.is_some_and(|summary| has_summary_body(document, summary))
            {
                return None;
            }

            Some(Violation::new(
                document.relative_path.clone(),
                first_h2.map_or_else(
                    || {
                        document
                            .headings
                            .iter()
                            .find(|heading| heading.level == 1)
                            .map_or(1, |heading| heading.line + 1)
                    },
                    |heading| heading.line,
                ),
                "markdown.tldr",
                "long-form guide must begin its level-two sections with a non-empty `TL;DR`",
                "add `## TL;DR` and a concise rendered summary immediately after the title and any status metadata",
                DOCUMENTATION_POLICY,
            ))
        })
        .collect()
}

fn has_summary_body(document: &Document, summary_index: usize) -> bool {
    let summary = &document.headings[summary_index];
    let end = document.headings[summary_index + 1..]
        .iter()
        .find(|heading| heading.level <= 2)
        .map_or_else(|| document.body.lines().count() + 1, |heading| heading.line);
    document.body.lines().enumerate().any(|(index, source)| {
        let line = index + 1;
        line > summary.line
            && line < end
            && !source.trim_start().starts_with('#')
            && document.first_raw_html_between(line, line + 1).is_none()
            && document
                .visible_line_text(line)
                .is_some_and(|text| text.chars().any(char::is_alphanumeric))
    })
}

fn no_prose_before_summary(document: &Document, title_line: usize, summary_line: usize) -> bool {
    let mut in_comment = false;
    document
        .body
        .lines()
        .skip(title_line)
        .take(summary_line.saturating_sub(title_line + 1))
        .map(|line| without_html_comments(line, &mut in_comment))
        .all(|line| line.trim().is_empty() || is_status_metadata(line.trim()))
}

fn without_html_comments(line: &str, in_comment: &mut bool) -> String {
    let mut visible = String::new();
    let mut remaining = line;
    loop {
        if *in_comment {
            let Some(end) = remaining.find("-->") else {
                return visible;
            };
            *in_comment = false;
            remaining = &remaining[end + 3..];
            continue;
        }
        let Some(start) = remaining.find("<!--") else {
            visible.push_str(remaining);
            return visible;
        };
        visible.push_str(&remaining[..start]);
        *in_comment = true;
        remaining = &remaining[start + 4..];
    }
}

fn is_status_metadata(line: &str) -> bool {
    line.starts_with("Status:") || line.starts_with("**Status:**") || line.starts_with("- Status:")
}

fn discover(root: &Path, directory: &Path, paths: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);

    for entry in entries {
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            if !is_excluded_directory(&entry.file_name()) {
                discover(root, &path, paths)?;
            }
        } else if file_type.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        {
            paths.push(path);
        }
    }

    debug_assert!(directory.starts_with(root));
    Ok(())
}

fn is_excluded_directory(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| EXCLUDED_DIRECTORIES.contains(&name))
}

fn relative_path(root: &Path, path: &Path) -> Result<String, String> {
    path.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .map_err(|error| {
            format!(
                "Markdown document {} is outside repository root {}: {error}",
                path.display(),
                root.display()
            )
        })
}

fn requires_tldr(document: &Document) -> bool {
    if is_tldr_exempt(&document.relative_path) {
        return false;
    }

    document.word_count >= 800
        || document
            .headings
            .iter()
            .filter(|heading| heading.level == 2)
            .count()
            > 5
        || is_long_form_guide(&document.relative_path)
}

fn is_tldr_exempt(path: &str) -> bool {
    path.starts_with("docs/adr/")
        || path.starts_with(".github/ISSUE_TEMPLATE/")
        || path == ".github/ISSUE_TEMPLATE.md"
        || path.starts_with(".github/PULL_REQUEST_TEMPLATE/")
        || path == ".github/PULL_REQUEST_TEMPLATE.md"
}

fn is_long_form_guide(path: &str) -> bool {
    let normalized = path.to_ascii_lowercase();
    normalized.contains("security")
        || normalized.starts_with("docs/architecture/")
        || normalized.starts_with("docs/operations/")
        || normalized.contains("migration")
        || normalized.contains("operational")
        || normalized.contains("end-to-end")
}

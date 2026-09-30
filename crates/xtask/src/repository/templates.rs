mod issue;
mod pull_request;

#[cfg(test)]
mod tests;

use super::{ISSUE_POLICY, PULL_REQUEST_POLICY, Repository, Violation};

const CONTRIBUTING: &str = "CONTRIBUTING.md";
const ISSUE_TEMPLATE: &str = ".github/ISSUE_TEMPLATE/implementation.md";
const PULL_REQUEST_TEMPLATE: &str = ".github/PULL_REQUEST_TEMPLATE.md";
const ISSUE_FRONT_MATTER: [&str; 7] = [
    "---",
    "name: Implementation task",
    "about: Propose one coherent, reviewable capability",
    "title: \"\"",
    "labels: \"\"",
    "assignees: \"\"",
    "---",
];

pub(super) fn check(repository: &Repository) -> Vec<Violation> {
    let mut violations = issue::check(repository);
    violations.extend(pull_request::check(repository));
    violations
}

pub(super) fn fenced_block_after_heading(
    document: &crate::repository::markdown::Document,
    heading: &str,
    language: &str,
) -> Option<(String, usize)> {
    let (start, end) = section_lines(document, heading)?;
    document
        .code_blocks()
        .iter()
        .find(|block| block.line > start && block.line < end && block.info.trim() == language)
        .map(|block| {
            (
                block.body.trim_end_matches(['\r', '\n']).to_owned(),
                block.line + 1,
            )
        })
}

pub(super) fn fenced_commands_under_heading(
    document: &crate::repository::markdown::Document,
    heading: &str,
) -> Vec<String> {
    let Some((start, end)) = section_lines(document, heading) else {
        return Vec::new();
    };
    let mut commands = Vec::new();
    for block in document
        .code_blocks()
        .iter()
        .filter(|block| block.line > start && block.line < end)
    {
        for line in block.body.lines() {
            let command = line.trim();
            if command.starts_with("cargo ") && !commands.iter().any(|item| item == command) {
                commands.push(command.to_owned());
            }
        }
    }
    commands
}

fn section_lines(
    document: &crate::repository::markdown::Document,
    name: &str,
) -> Option<(usize, usize)> {
    let headings = document.headings();
    let (index, heading) = headings
        .iter()
        .enumerate()
        .find(|(_, heading)| heading.level == 2 && heading.text.trim() == name)?;
    let end = headings[index + 1..]
        .iter()
        .find(|candidate| candidate.level <= 2)
        .map_or_else(
            || document.body().lines().count() + 1,
            |candidate| candidate.line,
        );
    Some((heading.line, end))
}

pub(super) fn document_second_level_headings(
    document: &crate::repository::markdown::Document,
) -> Vec<String> {
    document
        .headings()
        .iter()
        .filter(|heading| heading.level == 2)
        .map(|heading| heading.text.trim().to_owned())
        .collect()
}

pub(super) fn heading_line(body: &str, heading: &str) -> Option<usize> {
    let marker = format!("## {heading}");
    body.lines()
        .position(|line| line == marker)
        .map(|index| index + 1)
}

pub(super) fn first_second_level(body: &str) -> usize {
    body.lines()
        .position(|line| line.starts_with("## "))
        .map_or(1, |index| index + 1)
}

pub(super) fn count_visible_prefixed_lines(
    document: &crate::repository::markdown::Document,
    prefix: &str,
) -> usize {
    document
        .body()
        .lines()
        .enumerate()
        .filter(|(index, _)| {
            document
                .visible_line_text(index + 1)
                .is_some_and(|line| line.trim().starts_with(prefix))
        })
        .count()
}

pub(super) fn has_canonical_milestone_field(
    document: &crate::repository::markdown::Document,
) -> bool {
    document.body().lines().enumerate().any(|(index, _)| {
        document
            .visible_line_text(index + 1)
            .is_some_and(|line| line.trim() == "Milestone: MNN - Outcome")
    })
}

#[cfg(test)]
pub(super) fn visible_body_contains(
    document: &crate::repository::markdown::Document,
    marker: &str,
) -> bool {
    document.visible_text_contains(marker)
}

pub(super) fn visible_section_contains(
    document: &crate::repository::markdown::Document,
    heading: &str,
    marker: &str,
) -> bool {
    let Some((start, end)) = section_lines(document, heading) else {
        return false;
    };
    document.body().lines().enumerate().any(|(index, _)| {
        let line = index + 1;
        line > start
            && line < end
            && document
                .visible_line_text(line)
                .is_some_and(|text| text.contains(marker))
    })
}

pub(super) fn visible_section_cargo_commands(
    document: &crate::repository::markdown::Document,
    heading: &str,
) -> Vec<String> {
    let Some((start, end)) = section_lines(document, heading) else {
        return Vec::new();
    };
    let lines: Vec<_> = document.body().lines().collect();
    document
        .table_rows()
        .iter()
        .filter(|row| row.line > start && row.line < end)
        .filter_map(|row| lines.get(row.line - 1))
        .filter_map(|line| line.trim().strip_prefix('|'))
        .filter_map(|row_source| row_source.split_once('|').map(|(cell, _)| cell.trim()))
        .filter_map(exact_inline_code)
        .filter(|command| command.starts_with("cargo "))
        .map(ToOwned::to_owned)
        .collect()
}

fn exact_inline_code(source: &str) -> Option<&str> {
    let code = source.strip_prefix('`')?.strip_suffix('`')?;
    (!code.is_empty() && !code.contains('`')).then_some(code)
}

pub(super) fn has_required_issue_front_matter(body: &str) -> bool {
    body.lines()
        .take(ISSUE_FRONT_MATTER.len())
        .eq(ISSUE_FRONT_MATTER)
}

pub(super) fn visible_section_body_lines(
    document: &crate::repository::markdown::Document,
    heading: &str,
) -> Vec<String> {
    let Some((start, end)) = section_lines(document, heading) else {
        return Vec::new();
    };
    document
        .body()
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line_number = index + 1;
            let line = line.trim();
            (line_number > start
                && line_number < end
                && !line.is_empty()
                && document.line_is_visible(line_number))
            .then(|| line.to_owned())
        })
        .collect()
}

pub(super) fn visible_section_checkbox_lines(
    document: &crate::repository::markdown::Document,
    heading: &str,
) -> Vec<String> {
    visible_section_body_lines(document, heading)
        .into_iter()
        .filter(|line| line.starts_with("- [ ] "))
        .collect()
}

pub(super) fn template_body(body: &str) -> &str {
    let Some(after_opening) = body.strip_prefix("---") else {
        return body;
    };
    let Some((_, template)) = after_opening.split_once("\n---") else {
        return body;
    };
    template.trim_start_matches(['\r', '\n'])
}

pub(super) fn normalize_markdown(body: &str) -> String {
    body.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

pub(super) fn issue_violation(
    path: impl Into<String>,
    line: usize,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Violation {
    Violation::new(path, line, "template.issue", message, fix, ISSUE_POLICY)
}

pub(super) fn pull_request_violation(
    line: usize,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Violation {
    pull_request_policy_violation(PULL_REQUEST_TEMPLATE, line, message, fix)
}

pub(super) fn pull_request_policy_violation(
    path: impl Into<String>,
    line: usize,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Violation {
    Violation::new(
        path,
        line,
        "template.pull-request",
        message,
        fix,
        PULL_REQUEST_POLICY,
    )
}

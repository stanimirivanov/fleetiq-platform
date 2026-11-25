use std::collections::{BTreeMap, BTreeSet};

use super::{MILESTONE_POLICY, Repository, Violation};

const ROADMAP: &str = "docs/roadmap/milestones.md";
const LATEST_PUBLISHED_MILESTONE: usize = 9;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Milestone {
    number: usize,
    title: String,
    line: usize,
}

pub(super) fn check(repository: &Repository) -> Vec<Violation> {
    let Some(document) = repository.document(ROADMAP) else {
        return vec![milestone_violation(
            1,
            "milestone.roadmap",
            "milestone roadmap is missing",
            "restore the canonical roadmap with its index and boundary sections",
        )];
    };

    let (index, mut violations) = read_index(document);
    let (boundaries, boundary_violations) = read_boundaries(document);
    violations.extend(boundary_violations);
    violations.extend(validate_sequence(&index));
    violations.extend(validate_index_and_boundaries(&index, &boundaries));
    violations
}

fn read_index(
    document: &crate::repository::markdown::Document,
) -> (Vec<Milestone>, Vec<Violation>) {
    let mut milestones = Vec::new();
    let mut violations = Vec::new();
    let Some((start, end, duplicate)) = section_lines(document, "Milestone index") else {
        return (
            milestones,
            vec![milestone_violation(
                1,
                "milestone.index",
                "roadmap has no `Milestone index` section",
                "restore the canonical milestone table",
            )],
        );
    };
    if let Some(line) = duplicate {
        violations.push(milestone_violation(
            line,
            "milestone.index",
            "roadmap must contain exactly one rendered `Milestone index` section",
            "remove the duplicate section and keep one canonical milestone table",
        ));
    }
    if let Some(line) = document.first_raw_html_between(start, end) {
        violations.push(milestone_violation(
            line,
            "milestone.index",
            "milestone index uses raw HTML around policy-bearing content",
            "express milestone titles and short messages with plain Markdown",
        ));
    }

    for row in document
        .table_rows()
        .iter()
        .filter(|row| row.line > start && row.line < end)
    {
        let Some(first_cell) = row.cells.first().map(|cell| cell.trim()) else {
            continue;
        };
        if first_cell == "Milestone" {
            continue;
        }
        match parse_title(first_cell, row.line) {
            Some(milestone) => {
                if row
                    .cells
                    .get(1)
                    .is_none_or(|cell| !cell.chars().any(char::is_alphanumeric))
                {
                    violations.push(milestone_violation(
                        row.line,
                        "milestone.index",
                        format!("{} has no short outcome message", milestone.title),
                        "add a concise message describing what becomes true when the milestone completes",
                    ));
                }
                milestones.push(milestone);
            }
            None => violations.push(milestone_violation(
                row.line,
                "milestone.title",
                format!("index value `{first_cell}` does not match `MNN - Outcome`"),
                "use a two-digit positive number, one space on each side of the hyphen, and a non-empty outcome",
            )),
        }
    }
    if milestones.is_empty() {
        violations.push(milestone_violation(
            start,
            "milestone.index",
            "milestone index contains no milestone rows",
            "add at least `M01 - Outcome` and its short message",
        ));
    }
    (milestones, violations)
}

fn read_boundaries(
    document: &crate::repository::markdown::Document,
) -> (Vec<Milestone>, Vec<Violation>) {
    let mut milestones = Vec::new();
    let mut violations = Vec::new();
    let Some((start, end, duplicate)) = section_lines(document, "Milestone boundaries") else {
        return (
            milestones,
            vec![milestone_violation(
                1,
                "milestone.boundaries",
                "roadmap has no `Milestone boundaries` section",
                "restore one boundary section for every indexed milestone",
            )],
        );
    };
    if let Some(line) = duplicate {
        violations.push(milestone_violation(
            line,
            "milestone.boundaries",
            "roadmap must contain exactly one rendered `Milestone boundaries` section",
            "remove the duplicate section and keep one canonical set of boundary headings",
        ));
    }
    if let Some(line) = document.first_raw_html_between(start, end) {
        violations.push(milestone_violation(
            line,
            "milestone.boundaries",
            "milestone boundaries use raw HTML around policy-bearing content",
            "express milestone boundary headings and outcomes with plain Markdown",
        ));
    }

    for heading in document
        .headings()
        .iter()
        .filter(|heading| heading.level == 3 && heading.line > start && heading.line < end)
    {
        match parse_title(heading.text.trim(), heading.line) {
            Some(milestone) => milestones.push(milestone),
            None => violations.push(milestone_violation(
                heading.line,
                "milestone.title",
                format!(
                    "boundary heading `{}` does not match `MNN - Outcome`",
                    heading.text
                ),
                "copy the exact indexed milestone title into the level-three heading",
            )),
        }
    }
    (milestones, violations)
}

fn validate_sequence(milestones: &[Milestone]) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, milestone) in milestones.iter().enumerate() {
        if !seen.insert(milestone.number) {
            violations.push(milestone_violation(
                milestone.line,
                "milestone.sequence",
                format!(
                    "M{:02} appears more than once in the index",
                    milestone.number
                ),
                "keep one row for every published milestone number",
            ));
        }
        let expected = index + 1;
        if milestone.number != expected {
            violations.push(milestone_violation(
                milestone.line,
                "milestone.sequence",
                format!(
                    "milestone sequence expected M{expected:02}, found M{:02}",
                    milestone.number
                ),
                "start at M01 and keep published milestone numbers contiguous and ordered",
            ));
        }
    }
    let latest = milestones.last().map_or(0, |milestone| milestone.number);
    if latest != LATEST_PUBLISHED_MILESTONE {
        violations.push(milestone_violation(
            milestones.last().map_or(1, |milestone| milestone.line),
            "milestone.sequence",
            format!(
                "latest milestone is M{latest:02}; repository policy records M{LATEST_PUBLISHED_MILESTONE:02}"
            ),
            "restore deleted history, or advance `LATEST_PUBLISHED_MILESTONE` only while publishing the next milestone",
        ));
    }
    violations
}

fn validate_index_and_boundaries(index: &[Milestone], boundaries: &[Milestone]) -> Vec<Violation> {
    let indexed: BTreeMap<_, _> = index
        .iter()
        .map(|milestone| (milestone.number, milestone))
        .collect();
    let mut boundary_counts: BTreeMap<usize, Vec<&Milestone>> = BTreeMap::new();
    for boundary in boundaries {
        boundary_counts
            .entry(boundary.number)
            .or_default()
            .push(boundary);
    }

    let mut violations = Vec::new();
    for milestone in index {
        match boundary_counts.get(&milestone.number).map(Vec::as_slice) {
            None | Some([]) => violations.push(milestone_violation(
                milestone.line,
                "milestone.boundaries",
                format!("{} has no boundary section", milestone.title),
                "add a level-three boundary heading with the exact indexed title and describe its outcome",
            )),
            Some([boundary]) if boundary.title != milestone.title => {
                violations.push(milestone_violation(
                    boundary.line,
                    "milestone.boundaries",
                    format!("boundary title `{}` differs from index title `{}`", boundary.title, milestone.title),
                    "make the boundary heading exactly match the published index title",
                ));
            }
            Some([_]) => {}
            Some(duplicates) => violations.push(milestone_violation(
                duplicates[1].line,
                "milestone.boundaries",
                format!("M{:02} has more than one boundary section", milestone.number),
                "keep exactly one boundary section for each indexed milestone",
            )),
        }
    }
    for boundary in boundaries {
        if !indexed.contains_key(&boundary.number) {
            violations.push(milestone_violation(
                boundary.line,
                "milestone.boundaries",
                format!("{} has no index row", boundary.title),
                "add the published milestone to the index or remove an unpublished stale section",
            ));
        }
    }
    violations
}

fn parse_title(value: &str, line: usize) -> Option<Milestone> {
    let (identifier, outcome) = value.split_once(" - ")?;
    let number = identifier.strip_prefix('M')?;
    if number.len() != 2
        || !number.bytes().all(|byte| byte.is_ascii_digit())
        || !outcome.chars().any(char::is_alphanumeric)
        || outcome != outcome.trim()
    {
        return None;
    }
    let number = number.parse().ok()?;
    (number > 0).then(|| Milestone {
        number,
        title: value.to_owned(),
        line,
    })
}

fn section_lines(
    document: &crate::repository::markdown::Document,
    name: &str,
) -> Option<(usize, usize, Option<usize>)> {
    let headings = document.headings();
    let mut matching = headings
        .iter()
        .enumerate()
        .filter(|(_, heading)| heading.level == 2 && heading.text.trim() == name);
    let (index, heading) = matching.next()?;
    let duplicate = matching.next().map(|(_, heading)| heading.line);
    let end = headings[index + 1..]
        .iter()
        .find(|candidate| candidate.level <= 2)
        .map_or_else(
            || document.body().lines().count() + 1,
            |candidate| candidate.line,
        );
    Some((heading.line, end, duplicate))
}

fn milestone_violation(
    line: usize,
    rule: &'static str,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Violation {
    Violation::new(ROADMAP, line, rule, message, fix, MILESTONE_POLICY)
}

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use crate::repository::markdown::Document;

use super::{Record, Violation, adr_violation};

pub(super) fn read(document: &Document, lines: &[&str], record: &mut Record) -> Vec<Violation> {
    let mut reading_supersedes = false;
    let mut supersedes_seen = false;
    let mut replacement_seen = false;
    let (metadata_start, metadata_end) = metadata_lines(document, lines.len());
    let mut violations = Vec::new();
    if let Some(line) = document.first_raw_html_between(metadata_start, metadata_end) {
        violations.push(adr_violation(
            &record.path,
            line,
            "adr.supersession",
            "ADR lifecycle metadata uses raw HTML",
            "express lifecycle and supersession metadata with plain Markdown",
        ));
    }
    for (index, _) in lines.iter().enumerate() {
        let line_number = index + 1;
        if line_number <= metadata_start || line_number >= metadata_end {
            continue;
        }
        let Some(visible_line) = document.visible_line_text(line_number) else {
            continue;
        };
        let trimmed = visible_line.trim();
        let supersedes_labels = trimmed.matches("Supersedes:").count();
        if supersedes_labels > 0 {
            if supersedes_seen || supersedes_labels > 1 {
                violations.push(adr_violation(
                    &record.path,
                    line_number,
                    "adr.supersession",
                    "ADR declares `Supersedes` more than once",
                    "keep one supersession declaration and use continuation lines for additional references",
                ));
            }
            supersedes_seen = true;
        }
        if trimmed.matches("Superseded by:").count() > 1 {
            violations.push(adr_violation(
                &record.path,
                line_number,
                "adr.supersession",
                "ADR declares `Superseded by` more than once on one line",
                "keep one replacement declaration in lifecycle metadata",
            ));
        }
        if trimmed.starts_with("Status: superseded by") {
            set_superseded_by(
                record,
                trimmed,
                line_number,
                &mut replacement_seen,
                &mut violations,
            );
            reading_supersedes = false;
        } else if let Some((_, after_supersedes)) = trimmed.split_once("Supersedes:") {
            let (supersedes, superseded_by) = after_supersedes
                .split_once("Superseded by:")
                .map_or((after_supersedes, None), |(before, after)| {
                    (before, Some(after))
                });
            read_supersedes(record, supersedes, line_number, &mut violations);
            if let Some(replacement) = superseded_by {
                set_superseded_by(
                    record,
                    replacement,
                    line_number,
                    &mut replacement_seen,
                    &mut violations,
                );
            }
            reading_supersedes = superseded_by.is_none();
        } else if let Some((_, after)) = trimmed.split_once("Superseded by:") {
            set_superseded_by(
                record,
                after,
                line_number,
                &mut replacement_seen,
                &mut violations,
            );
            reading_supersedes = false;
        } else if reading_supersedes && !trimmed.is_empty() && !trimmed.starts_with('#') {
            record.supersedes.extend(references(trimmed));
        } else {
            reading_supersedes = false;
        }
    }
    record.supersedes.sort_unstable();
    record.supersedes.dedup();
    violations
}

fn metadata_lines(document: &Document, line_count: usize) -> (usize, usize) {
    let headings = document.headings();
    if let Some((index, status)) = headings
        .iter()
        .enumerate()
        .find(|(_, heading)| heading.level == 2 && heading.text.trim() == "Status")
    {
        let end = headings[index + 1..]
            .iter()
            .find(|heading| heading.level <= 2)
            .map_or(line_count + 1, |heading| heading.line);
        return (status.line, end);
    }

    let end = headings
        .iter()
        .find(|heading| heading.level == 2)
        .map_or(line_count + 1, |heading| heading.line);
    (0, end)
}

pub(super) fn validate(records: &BTreeMap<usize, Record>) -> Vec<Violation> {
    let known: BTreeSet<_> = records.keys().copied().collect();
    let mut violations = Vec::new();
    for record in records.values() {
        validate_direction(record, &mut violations);
        for reference in record.supersedes.iter().chain(record.superseded_by.iter()) {
            if !known.contains(reference) {
                violations.push(adr_violation(
                    &record.path,
                    1,
                    "adr.supersession",
                    format!("supersession metadata references missing ADR {reference:04}"),
                    "reference an existing indexed ADR",
                ));
            }
        }
        let is_superseded = record
            .status
            .as_deref()
            .is_some_and(|status| status.eq_ignore_ascii_case("superseded"));
        if is_superseded && record.superseded_by.is_none() {
            violations.push(adr_violation(
                &record.path,
                1,
                "adr.supersession",
                "superseded lifecycle status does not name a replacement ADR",
                "name the replacement in `Superseded by` metadata",
            ));
        }
        if let Some(replacement) = record.superseded_by
            && let Some(target) = records.get(&replacement)
            && !target.supersedes.contains(&record.number)
        {
            violations.push(adr_violation(
                &target.path,
                1,
                "adr.supersession",
                format!(
                    "ADR {replacement:04} replaces ADR {:04} but does not record the reciprocal `Supersedes` reference",
                    record.number
                ),
                "record both sides of the supersession relationship",
            ));
        }
    }
    violations
}

fn validate_direction(record: &Record, violations: &mut Vec<Violation>) {
    for superseded in &record.supersedes {
        if *superseded >= record.number {
            violations.push(adr_violation(
                &record.path,
                1,
                "adr.supersession",
                format!(
                    "ADR {:04} cannot supersede itself or later ADR {superseded:04}",
                    record.number
                ),
                "list only earlier decisions in `Supersedes` metadata",
            ));
        }
    }
    if let Some(replacement) = record.superseded_by
        && replacement <= record.number
    {
        violations.push(adr_violation(
            &record.path,
            1,
            "adr.supersession",
            format!(
                "ADR {:04} names itself or earlier ADR {replacement:04} as its replacement",
                record.number
            ),
            "name exactly one later decision in `Superseded by` metadata",
        ));
    }
}

pub(super) fn references(value: &str) -> Vec<usize> {
    let words: Vec<_> = value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '-'))
        .collect();
    let mut references = Vec::new();
    for (index, word) in words.iter().enumerate() {
        let candidate = word
            .strip_prefix("ADR-")
            .or_else(|| word.strip_prefix("ADR"))
            .filter(|number| !number.is_empty())
            .or_else(|| {
                (*word == "ADR")
                    .then(|| words.get(index + 1))
                    .flatten()
                    .copied()
            });
        if let Some(number) = candidate
            .filter(|number| number.len() == 4)
            .and_then(|number| number.parse().ok())
        {
            references.push(number);
        }
    }
    references
}

fn read_supersedes(record: &mut Record, value: &str, line: usize, violations: &mut Vec<Violation>) {
    let references = references(value);
    if references.is_empty() && !is_none(value) {
        violations.push(adr_violation(
            &record.path,
            line,
            "adr.supersession",
            "`Supersedes` must name an ADR or use canonical `none`",
            "use `Supersedes: none.` or name one or more four-digit ADR references",
        ));
    }
    record.supersedes.extend(references);
}

fn is_none(value: &str) -> bool {
    value
        .trim()
        .trim_end_matches('.')
        .eq_ignore_ascii_case("none")
}

fn set_superseded_by(
    record: &mut Record,
    value: &str,
    line: usize,
    replacement_seen: &mut bool,
    violations: &mut Vec<Violation>,
) {
    if *replacement_seen {
        violations.push(adr_violation(
            &record.path,
            line,
            "adr.supersession",
            "ADR declares `Superseded by` more than once",
            "keep one replacement declaration in lifecycle metadata",
        ));
    }
    *replacement_seen = true;
    let references = references(value);
    if references.is_empty() {
        if !is_none(value) {
            violations.push(adr_violation(
                &record.path,
                line,
                "adr.supersession",
                "`Superseded by` must name an ADR or use canonical `none`",
                "use `Superseded by: none.` or name one four-digit replacement ADR",
            ));
        }
        record.superseded_by = None;
        return;
    }
    if references.len() > 1 {
        violations.push(adr_violation(
            &record.path,
            line,
            "adr.supersession",
            "`Superseded by` names more than one replacement ADR",
            "name exactly one later replacement ADR",
        ));
    }
    record.superseded_by = references.into_iter().next();
}

use std::collections::BTreeMap;

use super::lifecycle::{legacy_status, status_lifecycle, valid_iso_date, valid_status_line};
use crate::repository::markdown::Document;

use super::{ADR_DIRECTORY, Record, Repository, Violation, adr_violation};

pub(super) const REQUIRED_SECTIONS: [&str; 5] = [
    "Status",
    "Context",
    "Decision",
    "Consequences",
    "Alternatives considered",
];
const LEGACY_SECTIONS: [&str; 4] = [
    "Context",
    "Decision",
    "Consequences",
    "Alternatives considered",
];

// ADRs 0001-0036 and the corrective ADR 0045 predate the current section-style
// lifecycle template. Their recorded metadata remains valid historical evidence,
// while newly added records must use `## Status`.
const LEGACY_LIST_METADATA_LAST_CONTIGUOUS: usize = 36;
const LEGACY_LIST_METADATA_EXCEPTIONS: &[usize] = &[45];
pub(super) fn read(repository: &Repository) -> (BTreeMap<usize, Record>, Vec<Violation>) {
    let mut records = BTreeMap::new();
    let mut violations = Vec::new();

    for (path, document) in &repository.documents {
        let Some(file_name) = path.strip_prefix(ADR_DIRECTORY) else {
            continue;
        };
        if file_name.contains('/') || matches!(file_name, "README.md" | "0000-template.md") {
            continue;
        }
        let Some(number) = parse_file_name(file_name) else {
            violations.push(adr_violation(
                path,
                1,
                "adr.filename",
                "ADR filename does not use a four-digit number and lowercase kebab-case title",
                "rename it to `NNNN-short-decision-title.md` and update the ADR index and inbound links",
            ));
            continue;
        };

        let mut record = Record {
            number,
            path: path.clone(),
            status: None,
            supersedes: Vec::new(),
            superseded_by: None,
        };
        let lines: Vec<_> = document.body().lines().collect();
        violations.extend(validate_record(document, &lines, &mut record));
        violations.extend(super::supersession::read(document, &lines, &mut record));
        if let Some(previous) = records.insert(number, record) {
            violations.push(adr_violation(
                path,
                1,
                "adr.number",
                format!("ADR number duplicates `{}`", previous.path),
                "renumber the unpublished ADR to the lowest unused number and update its references",
            ));
        }
    }

    (records, violations)
}

fn validate_record(document: &Document, lines: &[&str], record: &mut Record) -> Vec<Violation> {
    let mut violations = Vec::new();
    if !has_canonical_heading(document, record.number) {
        violations.push(adr_violation(
            &record.path,
            1,
            "adr.heading",
            format!(
                "first heading does not match `# ADR {:04}: Decision title`",
                record.number
            ),
            "make the first line use the filename's ADR number and a non-empty title",
        ));
    }

    let sections = second_level_sections(document);
    let has_status_section = sections.iter().any(|(name, _)| name == "Status");
    let required = if has_status_section {
        REQUIRED_SECTIONS.as_slice()
    } else {
        LEGACY_SECTIONS.as_slice()
    };
    violations.extend(validate_plain_required_headings(
        document,
        &record.path,
        required,
    ));
    violations.extend(validate_section_order(&record.path, &sections, required));

    if has_status_section {
        let status_line = sections
            .iter()
            .find_map(|(name, line)| (name == "Status").then_some(*line))
            .expect("status section was found");
        if let Some((body, line)) = first_section_body(document, lines, status_line) {
            record.status = status_lifecycle(body).map(ToOwned::to_owned);
            if !valid_status_line(body) {
                violations.push(adr_violation(
                    &record.path,
                    line,
                    "adr.status",
                    "status must begin with a supported lifecycle value followed by a calendar date",
                    "use `Proposed, D Month YYYY.`, `Accepted, D Month YYYY.`, `Rejected, D Month YYYY.`, `Deprecated, D Month YYYY.`, or `Superseded, D Month YYYY.`",
                ));
            }
        } else {
            violations.push(adr_violation(
                &record.path,
                status_line,
                "adr.status",
                "status section is empty",
                "add a supported lifecycle value and decision date below `## Status`",
            ));
        }
    } else {
        violations.extend(validate_legacy_metadata(document, lines, record));
        if !allows_legacy_list_metadata(record.number) {
            violations.push(adr_violation(
                &record.path,
                1,
                "adr.status",
                "ADR uses the historical list-style lifecycle metadata",
                "add the current `## Status` section with lifecycle, date, and supersession metadata",
            ));
        }
    }
    violations
}

pub(super) fn has_canonical_heading(document: &Document, number: usize) -> bool {
    let expected_prefix = format!("ADR {number:04}: ");
    document.headings().first().is_some_and(|heading| {
        heading.level == 1
            && heading.line == 1
            && heading.text.starts_with(&expected_prefix)
            && heading.text[expected_prefix.len()..]
                .chars()
                .any(char::is_alphanumeric)
    })
}

pub(super) fn validate_plain_required_headings(
    document: &Document,
    path: &str,
    required_sections: &[&str],
) -> Vec<Violation> {
    document
        .headings()
        .iter()
        .filter(|heading| {
            (heading.level == 1
                || (heading.level == 2 && required_sections.contains(&heading.text.trim())))
                && document
                    .first_raw_html_between(heading.line, heading.line + 1)
                    .is_some()
        })
        .map(|heading| {
            adr_violation(
                path,
                heading.line,
                "adr.sections",
                format!("required heading `{}` uses raw HTML", heading.text.trim()),
                "write required ADR headings with plain Markdown",
            )
        })
        .collect()
}

pub(super) fn allows_legacy_list_metadata(number: usize) -> bool {
    number <= LEGACY_LIST_METADATA_LAST_CONTIGUOUS
        || LEGACY_LIST_METADATA_EXCEPTIONS.contains(&number)
}

pub(super) fn validate_section_order(
    path: &str,
    sections: &[(String, usize)],
    required: &[&str],
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut previous = 0;
    for required_name in required {
        let matches: Vec<_> = sections
            .iter()
            .filter(|(name, _)| name == required_name)
            .collect();
        if matches.len() != 1 {
            violations.push(adr_violation(
                path,
                matches.first().map_or(1, |(_, line)| *line),
                "adr.sections",
                format!("required section `{required_name}` must appear exactly once"),
                "restore the canonical ADR sections while retaining any additional record-specific sections",
            ));
            continue;
        }
        let line = matches[0].1;
        if line < previous {
            violations.push(adr_violation(
                path,
                line,
                "adr.sections",
                format!("section `{required_name}` is out of canonical order"),
                "order Status, Context, Decision, Consequences, and Alternatives considered; extra sections may follow or appear between them",
            ));
        }
        previous = line;
    }
    violations
}

fn validate_legacy_metadata(
    document: &Document,
    lines: &[&str],
    record: &mut Record,
) -> Vec<Violation> {
    let first_section = lines
        .iter()
        .position(|line| line.starts_with("## "))
        .unwrap_or(lines.len());
    let status = metadata_value(document, &lines[..first_section], "Status");
    let date = metadata_value(document, &lines[..first_section], "Date");
    record.status = status
        .and_then(|(value, _)| legacy_status(value))
        .map(ToOwned::to_owned);

    let mut violations = Vec::new();
    if record.status.is_none() {
        violations.push(adr_violation(
            &record.path,
            status.map_or(1, |(_, line)| line),
            "adr.status",
            "legacy ADR metadata has no supported status",
            "retain historical metadata with accepted, proposed, rejected, deprecated, or `superseded by ADR NNNN`",
        ));
    }
    if !date.is_some_and(|(value, _)| valid_iso_date(value)) {
        violations.push(adr_violation(
            &record.path,
            date.map_or(1, |(_, line)| line),
            "adr.date",
            "legacy ADR metadata has no valid ISO decision date",
            "retain the historical list metadata with `- Date: YYYY-MM-DD`",
        ));
    }
    violations
}

pub(super) fn parse_file_name(file_name: &str) -> Option<usize> {
    let stem = file_name.strip_suffix(".md")?;
    let (number, slug) = stem.split_once('-')?;
    if number.len() != 4 || !number.bytes().all(|byte| byte.is_ascii_digit()) || !valid_slug(slug) {
        return None;
    }
    let number = number.parse().ok()?;
    (number > 0).then_some(number)
}

fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--")
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub(super) fn second_level_sections(document: &Document) -> Vec<(String, usize)> {
    document
        .headings()
        .iter()
        .filter(|heading| heading.level == 2)
        .map(|heading| (heading.text.trim().to_owned(), heading.line))
        .collect()
}

pub(super) fn first_section_body<'a>(
    document: &Document,
    lines: &'a [&str],
    heading_line: usize,
) -> Option<(&'a str, usize)> {
    let next_heading = document
        .headings()
        .iter()
        .find(|heading| heading.line > heading_line)
        .map_or(lines.len() + 1, |heading| heading.line);
    lines
        .iter()
        .enumerate()
        .skip(heading_line)
        .take_while(|(index, _)| index + 1 < next_heading)
        .find(|(index, line)| document.line_is_visible(index + 1) && !line.trim().is_empty())
        .map(|(index, line)| (line.trim(), index + 1))
}

fn metadata_value<'a>(
    document: &Document,
    lines: &'a [&str],
    field: &str,
) -> Option<(&'a str, usize)> {
    let prefix = format!("- {field}:");
    lines.iter().enumerate().find_map(|(index, line)| {
        document
            .line_is_visible(index + 1)
            .then(|| line.strip_prefix(&prefix))
            .flatten()
            .map(str::trim)
            .map(|value| (value, index + 1))
    })
}

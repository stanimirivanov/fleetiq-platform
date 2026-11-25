use std::collections::BTreeMap;

use super::{ADR_DIRECTORY, ADR_INDEX, Record, Repository, Violation, adr_violation};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct IndexRange {
    pub(super) first: usize,
    pub(super) last: usize,
    pub(super) target: String,
    pub(super) line: usize,
}

pub(super) fn validate(
    repository: &Repository,
    records: &BTreeMap<usize, Record>,
) -> Vec<Violation> {
    let Some(index) = repository.document(ADR_INDEX) else {
        return vec![adr_violation(
            ADR_INDEX,
            1,
            "adr.index",
            "ADR index is missing",
            "restore the index and register every decision exactly once",
        )];
    };
    let (ranges, mut violations) = index_ranges(index);
    let mut coverage: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let maximum_record = records.keys().next_back().copied().unwrap_or_default();

    for pair in ranges.windows(2) {
        if pair[1].first <= pair[0].first {
            violations.push(adr_violation(
                ADR_INDEX,
                pair[1].line,
                "adr.index",
                "ADR index ranges are not in ascending number order",
                "order singleton and grouped ranges by their first ADR number",
            ));
        }
    }

    for range in &ranges {
        if range.first == 0 || range.first > range.last {
            violations.push(adr_violation(
                ADR_INDEX,
                range.line,
                "adr.index",
                "ADR index range is invalid",
                "use a positive singleton or an ascending inclusive range such as `0002–0008`",
            ));
            continue;
        }
        if range.last > maximum_record {
            violations.push(adr_violation(
                ADR_INDEX,
                range.line,
                "adr.index",
                format!(
                    "ADR index range ends at {:04}, but the latest record is {maximum_record:04}",
                    range.last
                ),
                "end the range at the latest existing ADR and add future records when they are created",
            ));
        }
        for number in range.first..=range.last.min(maximum_record) {
            coverage.entry(number).or_default().push(range.line);
        }
        if let Some(record) = records.get(&range.first) {
            let expected = record.path.trim_start_matches(ADR_DIRECTORY);
            if range.target != expected {
                violations.push(adr_violation(
                    ADR_INDEX,
                    range.line,
                    "adr.index",
                    format!(
                        "range link targets `{}` instead of `{expected}`",
                        range.target
                    ),
                    "point each range link at its first numbered ADR",
                ));
            }
        }
    }

    validate_coverage(records, coverage, violations)
}

fn validate_coverage(
    records: &BTreeMap<usize, Record>,
    coverage: BTreeMap<usize, Vec<usize>>,
    mut violations: Vec<Violation>,
) -> Vec<Violation> {
    for (number, lines) in &coverage {
        if lines.len() > 1 {
            violations.push(adr_violation(
                ADR_INDEX,
                lines[1],
                "adr.index",
                format!("ADR {number:04} is covered by more than one index row"),
                "make the grouped ranges disjoint",
            ));
        }
    }
    for (number, record) in records {
        match coverage.get(number).map(Vec::as_slice) {
            None | Some([]) => violations.push(adr_violation(
                &record.path,
                1,
                "adr.index",
                format!("ADR {number:04} is not covered by the index"),
                "add it to exactly one singleton or inclusive range in the ADR index",
            )),
            Some(_) => {}
        }
    }
    for (number, lines) in coverage {
        if !records.contains_key(&number) {
            violations.push(adr_violation(
                ADR_INDEX,
                lines[0],
                "adr.index",
                format!("index covers ADR {number:04}, but no such record exists"),
                "restore the historical record or narrow an unpublished range",
            ));
        }
    }
    violations
}

pub(super) fn index_ranges(
    document: &crate::repository::markdown::Document,
) -> (Vec<IndexRange>, Vec<Violation>) {
    let index_headings: Vec<_> = document
        .headings()
        .iter()
        .enumerate()
        .filter(|(_, heading)| heading.level == 2 && heading.text.trim() == "Index")
        .collect();
    if index_headings.len() != 1 {
        return (
            Vec::new(),
            vec![adr_violation(
                ADR_INDEX,
                index_headings
                    .first()
                    .map_or(1, |(_, heading)| heading.line),
                "adr.index",
                "ADR index must contain exactly one rendered `Index` section",
                "restore one `## Index` heading around the canonical decision table",
            )],
        );
    }
    let (heading_index, heading) = index_headings[0];
    let end = document.headings()[heading_index + 1..]
        .iter()
        .find(|candidate| candidate.level <= 2)
        .map_or_else(
            || document.body().lines().count() + 1,
            |candidate| candidate.line,
        );
    let mut ranges = Vec::new();
    let mut violations = Vec::new();
    if let Some(line) = document.first_raw_html_between(heading.line, end) {
        violations.push(adr_violation(
            ADR_INDEX,
            line,
            "adr.index",
            "ADR index uses raw HTML around policy-bearing content",
            "express the Index heading and decision ranges with plain Markdown",
        ));
    }
    for (index, line) in document.body().lines().enumerate() {
        let line_number = index + 1;
        if line_number <= heading.line
            || line_number >= end
            || !document.line_is_visible(line_number)
        {
            continue;
        }
        if let Some(range) = parse_index_range(line, line_number) {
            ranges.push(range);
        } else if line.trim().starts_with("| [") && !line.contains("[Range]") {
            violations.push(adr_violation(
                ADR_INDEX,
                line_number,
                "adr.index",
                "ADR index row has a malformed range or link",
                "use a four-digit singleton or inclusive range linked to its first ADR",
            ));
        }
    }
    (ranges, violations)
}

pub(super) fn parse_index_range(row: &str, line_number: usize) -> Option<IndexRange> {
    let trimmed = row.trim();
    let link_body = trimmed.strip_prefix("| [")?;
    let (label, rest) = link_body.split_once("](")?;
    let (target, _) = rest.split_once(')')?;
    if label == "Range" {
        return None;
    }
    let (first, last) =
        if let Some((first, last)) = label.split_once('–').or_else(|| label.split_once('-')) {
            if !valid_number_label(first) || !valid_number_label(last) {
                return None;
            }
            (first.parse().ok()?, last.parse().ok()?)
        } else {
            if !valid_number_label(label) {
                return None;
            }
            let number = label.parse().ok()?;
            (number, number)
        };
    Some(IndexRange {
        first,
        last,
        target: target.to_owned(),
        line: line_number,
    })
}

fn valid_number_label(value: &str) -> bool {
    value.len() == 4 && value.bytes().all(|byte| byte.is_ascii_digit())
}

mod index;
mod lifecycle;
mod record;
mod supersession;
mod template;

use std::collections::BTreeMap;

use super::{ADR_POLICY, Repository, Violation};

const ADR_DIRECTORY: &str = "docs/adr/";
const ADR_INDEX: &str = "docs/adr/README.md";
const LATEST_PUBLISHED_ADR: usize = 72;

#[derive(Debug)]
struct Record {
    number: usize,
    path: String,
    status: Option<String>,
    supersedes: Vec<usize>,
    superseded_by: Option<usize>,
}

pub(super) fn check(repository: &Repository) -> Vec<Violation> {
    let (records, mut violations) = record::read(repository);
    violations.extend(template::validate(repository));
    violations.extend(validate_sequence(&records));
    violations.extend(index::validate(repository, &records));
    violations.extend(supersession::validate(&records));
    violations
}

fn validate_sequence(records: &BTreeMap<usize, Record>) -> Vec<Violation> {
    let mut violations = Vec::new();
    for (index, (number, record)) in records.iter().enumerate() {
        let expected = index + 1;
        if *number != expected {
            violations.push(adr_violation(
                &record.path,
                1,
                "adr.sequence",
                format!("ADR sequence expected {expected:04} before {number:04}"),
                "restore the missing historical record or renumber only unpublished ADRs and their references",
            ));
            break;
        }
    }
    let latest = records.keys().next_back().copied().unwrap_or_default();
    if latest != LATEST_PUBLISHED_ADR {
        violations.push(adr_violation(
            ADR_INDEX,
            1,
            "adr.sequence",
            format!(
                "latest ADR is {latest:04}; repository policy records {LATEST_PUBLISHED_ADR:04}"
            ),
            "restore deleted history, or advance `LATEST_PUBLISHED_ADR` only while adding and indexing the next decision",
        ));
    }
    violations
}

fn adr_violation(
    path: impl Into<String>,
    line: usize,
    rule: &'static str,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Violation {
    Violation::new(path, line, rule, message, fix, ADR_POLICY)
}

#[cfg(test)]
mod tests;

use crate::repository::markdown::Document;

use super::record::{
    REQUIRED_SECTIONS, first_section_body, second_level_sections, validate_plain_required_headings,
    validate_section_order,
};
use super::{Repository, Violation, adr_violation};

const ADR_TEMPLATE: &str = "docs/adr/0000-template.md";

pub(super) fn validate(repository: &Repository) -> Vec<Violation> {
    let Some(document) = repository.document(ADR_TEMPLATE) else {
        return vec![adr_violation(
            ADR_TEMPLATE,
            1,
            "adr.template",
            "canonical ADR template is missing",
            "restore the numbered decision template",
        )];
    };
    validate_document(document)
}

pub(super) fn validate_document(document: &Document) -> Vec<Violation> {
    let mut violations =
        validate_plain_required_headings(document, ADR_TEMPLATE, REQUIRED_SECTIONS.as_slice());
    violations.extend(validate_section_order(
        ADR_TEMPLATE,
        &second_level_sections(document),
        REQUIRED_SECTIONS.as_slice(),
    ));
    let title_is_canonical = document.headings().first().is_some_and(|heading| {
        heading.level == 1
            && heading.line == 1
            && heading.text.trim() == "ADR NNNN: Short decision title"
    });
    if !title_is_canonical {
        violations.push(adr_violation(
            ADR_TEMPLATE,
            1,
            "adr.template",
            "canonical ADR template has no placeholder decision title",
            "restore `# ADR NNNN: Short decision title` as the first heading",
        ));
    }

    let lines: Vec<_> = document.body().lines().collect();
    let status_line = document
        .headings()
        .iter()
        .find(|heading| heading.level == 2 && heading.text.trim() == "Status")
        .map(|heading| heading.line);
    if status_line.and_then(|line| first_section_body(document, &lines, line).map(|item| item.0))
        != Some("Proposed, DD Month YYYY.")
    {
        violations.push(adr_violation(
            ADR_TEMPLATE,
            status_line.unwrap_or(1),
            "adr.template",
            "canonical ADR template has no lifecycle and date placeholder",
            "restore `Proposed, DD Month YYYY.` below the Status heading",
        ));
    }
    let status_end = status_line.and_then(|line| {
        document
            .headings()
            .iter()
            .find(|heading| heading.line > line && heading.level <= 2)
            .map(|heading| heading.line)
    });
    if let Some(start) = status_line {
        let end = status_end.unwrap_or(lines.len() + 1);
        if let Some(line) = document.first_raw_html_between(start + 1, end) {
            violations.push(adr_violation(
                ADR_TEMPLATE,
                line,
                "adr.template",
                "canonical ADR template lifecycle metadata uses raw HTML",
                "express lifecycle and supersession placeholders with plain Markdown",
            ));
        }
    }
    let has_supersession_placeholder = status_line.is_some_and(|start| {
        let end = status_end.unwrap_or(lines.len() + 1);
        lines.iter().enumerate().any(|(index, line)| {
            let line_number = index + 1;
            line_number > start
                && line_number < end
                && document.line_is_visible(line_number)
                && line.trim() == "Supersedes: none. Superseded by: none."
        })
    });
    if !has_supersession_placeholder {
        violations.push(adr_violation(
            ADR_TEMPLATE,
            status_line.unwrap_or(1),
            "adr.template",
            "canonical ADR template has no supersession placeholders",
            "restore `Supersedes: none. Superseded by: none.` in the Status section",
        ));
    }
    violations
}

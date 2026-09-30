use super::*;

#[test]
fn parses_canonical_adr_names() {
    assert_eq!(
        record::parse_file_name("0067-enforce-repository-policy.md"),
        Some(67)
    );
    assert_eq!(record::parse_file_name("67-short.md"), None);
    assert_eq!(record::parse_file_name("0067-Uppercase.md"), None);
    assert_eq!(record::parse_file_name("0067-double--dash.md"), None);
}

#[test]
fn requires_a_rendered_adr_title_on_the_first_line() {
    let valid = crate::repository::markdown::parse_fragment(
        "docs/adr/0068-valid.md",
        "# ADR 0068: Visible title\n",
    );
    let hidden = crate::repository::markdown::parse_fragment(
        "docs/adr/0068-hidden.md",
        "# ADR 0068: <!-- hidden title -->\n",
    );
    let raw_html = crate::repository::markdown::parse_fragment(
        "docs/adr/0068-html.md",
        "# ADR 0068: <span hidden>Hidden title</span>\n",
    );
    let zero_width = crate::repository::markdown::parse_fragment(
        "docs/adr/0068-zero-width.md",
        "# ADR 0068: \u{200b}\n",
    );

    assert!(record::has_canonical_heading(&valid, 68));
    assert!(!record::has_canonical_heading(&hidden, 68));
    assert!(record::has_canonical_heading(&raw_html, 68));
    assert!(!record::has_canonical_heading(&zero_width, 68));
    assert!(!record::validate_plain_required_headings(&raw_html, "example.md", &[]).is_empty());
}

#[test]
fn parses_singleton_and_grouped_index_rows() {
    assert_eq!(
        index::parse_index_range("| [0001](0001-first.md) | First |", 12),
        Some(index::IndexRange {
            first: 1,
            last: 1,
            target: "0001-first.md".to_owned(),
            line: 12,
        })
    );
    assert_eq!(
        index::parse_index_range("| [0002–0008](0002-second.md) | Group |", 13),
        Some(index::IndexRange {
            first: 2,
            last: 8,
            target: "0002-second.md".to_owned(),
            line: 13,
        })
    );
    assert_eq!(
        index::parse_index_range("| [1-999999999](0001-first.md) | Invalid |", 14),
        None
    );
}

#[test]
fn rejects_raw_html_in_the_adr_index() {
    let body = "# Decisions\n\n## <span hidden>Index</span>\n\n\
                | Range | Decisions |\n|:--|:--|\n\
                | [0001](0001-first.md) | First |\n";
    let document = crate::repository::markdown::parse_fragment("docs/adr/README.md", body);

    let (_, violations) = index::index_ranges(&document);

    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("ADR index uses raw HTML"))
    );
}

#[test]
fn validates_both_supported_date_forms() {
    assert!(lifecycle::valid_status_line("Accepted, 29 September 2026."));
    assert!(lifecycle::valid_iso_date("2026-09-29"));
    assert!(!lifecycle::valid_status_line("Accepted someday"));
    assert!(!lifecycle::valid_iso_date("2026-02-30"));
}

#[test]
fn limits_legacy_metadata_to_historical_records() {
    assert!(record::allows_legacy_list_metadata(36));
    assert!(record::allows_legacy_list_metadata(45));
    assert!(!record::allows_legacy_list_metadata(46));
    assert!(!record::allows_legacy_list_metadata(68));
}

#[test]
fn finds_hyphenated_and_spaced_adr_references() {
    assert_eq!(
        supersession::references("Supersedes: ADR-0042 and ADR0060."),
        vec![42, 60]
    );
}

#[test]
fn separates_same_line_supersedes_and_superseded_by_metadata() {
    let mut decision = Record {
        number: 51,
        path: "docs/adr/0051-example.md".to_owned(),
        status: Some("accepted".to_owned()),
        supersedes: Vec::new(),
        superseded_by: None,
    };

    let body = "# ADR 0051: Example\n\n## Status\n\nAccepted, 1 January 2026.\n\n\
                Supersedes: none. Superseded by: ADR 0053 for one decision.\n\n\
                ## Context\n\nContext.\n";
    let document = crate::repository::markdown::parse_fragment("docs/adr/0051-example.md", body);
    let lines: Vec<_> = body.lines().collect();

    let violations = supersession::read(&document, &lines, &mut decision);

    assert!(violations.is_empty());
    assert!(decision.supersedes.is_empty());
    assert_eq!(decision.superseded_by, Some(53));
}

#[test]
fn rejects_duplicate_same_line_superseded_by_metadata() {
    let mut decision = Record {
        number: 51,
        path: "docs/adr/0051-example.md".to_owned(),
        status: Some("accepted".to_owned()),
        supersedes: Vec::new(),
        superseded_by: None,
    };
    let body = "# ADR 0051: Example\n\n## Status\n\nAccepted, 1 January 2026.\n\n\
                Supersedes: none. Superseded by: none. Superseded by: ADR 0053.\n\n\
                ## Context\n\nContext.\n";
    let document = crate::repository::markdown::parse_fragment("docs/adr/0051-example.md", body);
    let lines: Vec<_> = body.lines().collect();

    let violations = supersession::read(&document, &lines, &mut decision);

    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("more than once on one line"))
    );
}

#[test]
fn rejects_duplicate_supersedes_declarations() {
    let mut decision = Record {
        number: 51,
        path: "docs/adr/0051-example.md".to_owned(),
        status: Some("accepted".to_owned()),
        supersedes: Vec::new(),
        superseded_by: None,
    };
    let body = "# ADR 0051: Example\n\n## Status\n\nAccepted, 1 January 2026.\n\n\
                Supersedes: none.\n\nSupersedes: ADR 0050. Superseded by: none.\n\n\
                ## Context\n\nContext.\n";
    let document = crate::repository::markdown::parse_fragment("docs/adr/0051-example.md", body);
    let lines: Vec<_> = body.lines().collect();

    let violations = supersession::read(&document, &lines, &mut decision);

    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("`Supersedes` more than once"))
    );
}

#[test]
fn ignores_supersession_text_outside_the_status_section() {
    let mut decision = Record {
        number: 51,
        path: "docs/adr/0051-example.md".to_owned(),
        status: Some("accepted".to_owned()),
        supersedes: Vec::new(),
        superseded_by: None,
    };
    let body = "# ADR 0051: Example\n\n## Status\n\nAccepted, 1 January 2026.\n\n\
                Supersedes: none. Superseded by: none.\n\n## Notes\n\n\
                Superseded by: ADR 0053.\n\n## Context\n\nContext.\n";
    let document = crate::repository::markdown::parse_fragment("docs/adr/0051-example.md", body);
    let lines: Vec<_> = body.lines().collect();

    let violations = supersession::read(&document, &lines, &mut decision);

    assert!(violations.is_empty());
    assert!(decision.supersedes.is_empty());
    assert_eq!(decision.superseded_by, None);
}

#[test]
fn rejects_malformed_supersession_values() {
    let mut decision = Record {
        number: 51,
        path: "docs/adr/0051-example.md".to_owned(),
        status: Some("accepted".to_owned()),
        supersedes: Vec::new(),
        superseded_by: None,
    };
    let body = "# ADR 0051: Example\n\n## Status\n\nAccepted, 1 January 2026.\n\n\
                Supersedes: unknown. Superseded by: ADR 53.\n\n\
                ## Context\n\nContext.\n";
    let document = crate::repository::markdown::parse_fragment("docs/adr/0051-example.md", body);
    let lines: Vec<_> = body.lines().collect();

    let violations = supersession::read(&document, &lines, &mut decision);

    assert_eq!(violations.len(), 2);
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("`Supersedes`"))
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("`Superseded by`"))
    );
}

#[test]
fn ignores_supersession_examples_after_context() {
    let body = "# ADR 0051: Example\n\n## Status\n\nAccepted, 1 January 2026.\n\n\
                Supersedes: none. Superseded by: none.\n\n## Context\n\n\
                ```text\nSuperseded by: ADR 0052.\n```\n\nSupersedes: ADR 0049.\n";
    let document = crate::repository::markdown::parse_fragment("docs/adr/0051-example.md", body);
    let lines: Vec<_> = body.lines().collect();
    let mut decision = Record {
        number: 51,
        path: "docs/adr/0051-example.md".to_owned(),
        status: Some("accepted".to_owned()),
        supersedes: Vec::new(),
        superseded_by: None,
    };

    let violations = supersession::read(&document, &lines, &mut decision);

    assert!(violations.is_empty());
    assert!(decision.supersedes.is_empty());
    assert_eq!(decision.superseded_by, None);
}

#[test]
fn ignores_supersession_metadata_hidden_in_inline_html() {
    let body = "# ADR 0051: Example\n\n## Status\n\nSuperseded, 1 January 2026.\n\n\
                Visible note. <!-- Superseded by: ADR 0053. -->\n\n\
                ## Context\n\nContext.\n";
    let document = crate::repository::markdown::parse_fragment("docs/adr/0051-example.md", body);
    let lines: Vec<_> = body.lines().collect();
    let mut decision = Record {
        number: 51,
        path: "docs/adr/0051-example.md".to_owned(),
        status: Some("superseded".to_owned()),
        supersedes: Vec::new(),
        superseded_by: None,
    };

    let violations = supersession::read(&document, &lines, &mut decision);

    assert!(violations.is_empty());
    assert_eq!(decision.superseded_by, None);
}

#[test]
fn rejects_raw_html_around_lifecycle_metadata() {
    let body = "# ADR 0051: Example\n\n## Status\n\nAccepted, 1 January 2026.\n\n\
                <span hidden>Superseded by: ADR 0053.</span>\n\n\
                ## Context\n\nContext.\n";
    let document = crate::repository::markdown::parse_fragment("docs/adr/0051-example.md", body);
    let lines: Vec<_> = body.lines().collect();
    let mut decision = Record {
        number: 51,
        path: "docs/adr/0051-example.md".to_owned(),
        status: Some("accepted".to_owned()),
        supersedes: Vec::new(),
        superseded_by: None,
    };

    let violations = supersession::read(&document, &lines, &mut decision);

    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("uses raw HTML"))
    );
}

#[test]
fn rejects_a_commented_out_adr_template() {
    let body = "# ADR NNNN: Short decision title\n\n<!--\n## Status\n\n\
                Proposed, DD Month YYYY.\n\nSupersedes: none. Superseded by: none.\n\n\
                ## Context\n\n## Decision\n\n## Consequences\n\n\
                ## Alternatives considered\n-->\n";
    let document = crate::repository::markdown::parse_fragment("docs/adr/0000-template.md", body);

    let violations = template::validate_document(&document);

    assert!(!violations.is_empty());
}

#[test]
fn rejects_raw_html_in_required_adr_template_headings() {
    let body = "# <span hidden>ADR NNNN: Short decision title</span>\n\n\
                ## Status\n\nProposed, DD Month YYYY.\n\n\
                Supersedes: none. Superseded by: none.\n\n\
                ## <span hidden>Context</span>\n\n## Decision\n\n## Consequences\n\n\
                ## Alternatives considered\n";
    let document = crate::repository::markdown::parse_fragment("docs/adr/0000-template.md", body);

    let violations = template::validate_document(&document);

    assert_eq!(
        violations
            .iter()
            .filter(|violation| violation.message.contains("uses raw HTML"))
            .count(),
        2
    );
}

#[test]
fn rejects_prose_before_the_adr_template_title() {
    let canonical = include_str!("../../../../../docs/adr/0000-template.md");
    let body = format!("Introduction.\n\n{canonical}");
    let document = crate::repository::markdown::parse_fragment("docs/adr/0000-template.md", &body);

    let violations = template::validate_document(&document);

    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("placeholder decision title"))
    );
}

#[test]
fn rejects_raw_html_in_adr_template_lifecycle_metadata() {
    let body = include_str!("../../../../../docs/adr/0000-template.md").replace(
        "Supersedes: none. Superseded by: none.",
        "Supersedes: none. Superseded by: none. <span hidden>instruction</span>",
    );
    let document = crate::repository::markdown::parse_fragment("docs/adr/0000-template.md", &body);

    let violations = template::validate_document(&document);

    assert!(violations.iter().any(|violation| {
        violation
            .message
            .contains("lifecycle metadata uses raw HTML")
    }));
}

#[test]
fn rejects_deleting_the_published_adr_tail() {
    let records = (1..LATEST_PUBLISHED_ADR)
        .map(|number| {
            (
                number,
                Record {
                    number,
                    path: format!("docs/adr/{number:04}-example.md"),
                    status: Some("accepted".to_owned()),
                    supersedes: Vec::new(),
                    superseded_by: None,
                },
            )
        })
        .collect();

    let violations = validate_sequence(&records);

    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("records 0067"))
    );
}

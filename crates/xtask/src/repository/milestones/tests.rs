use super::*;

#[test]
fn parses_canonical_title() {
    assert_eq!(
        parse_title("M01 - Decisions and Contracts", 4),
        Some(Milestone {
            number: 1,
            title: "M01 - Decisions and Contracts".to_owned(),
            line: 4,
        })
    );
}

#[test]
fn rejects_noncanonical_titles() {
    assert!(parse_title("M1 - Missing zero", 1).is_none());
    assert!(parse_title("M00 - Reserved", 1).is_none());
    assert!(parse_title("M01-No spaces", 1).is_none());
    assert!(parse_title("M01 -  Extra space", 1).is_none());
    assert!(parse_title("M01 - ", 1).is_none());
    assert!(parse_title("M01 - \u{200b}", 1).is_none());
}

#[test]
fn reads_the_rendered_milestone_table() {
    let body = "# Roadmap\n\n## Milestone index\n\n\
                | Milestone | Short message |\n|:--|:--|\n\
                | **M01 - First outcome** | Establish contracts. |\n\n\
                ## Milestone boundaries\n";
    let document = crate::repository::markdown::parse_fragment("docs/roadmap/milestones.md", body);

    let (milestones, violations) = read_index(&document);

    assert_eq!(
        milestones,
        vec![Milestone {
            number: 1,
            title: "M01 - First outcome".to_owned(),
            line: 7,
        }]
    );
    assert!(violations.is_empty());
}

#[test]
fn ignores_milestone_rows_hidden_in_code_blocks() {
    let body = "# Roadmap\n\n## Milestone index\n\n```markdown\n\
                | Milestone | Short message |\n|:--|:--|\n\
                | M01 - Hidden | Not a real row. |\n```\n\n\
                ## Milestone boundaries\n";
    let document = crate::repository::markdown::parse_fragment("docs/roadmap/milestones.md", body);

    let (milestones, violations) = read_index(&document);

    assert!(milestones.is_empty());
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("no milestone rows"))
    );
}

#[test]
fn rejects_a_milestone_message_present_only_in_a_comment() {
    let body = "# Roadmap\n\n## Milestone index\n\n\
                | Milestone | Short message |\n|:--|:--|\n\
                | M01 - Visible | <!-- hidden --> |\n\n\
                ## Milestone boundaries\n";
    let document = crate::repository::markdown::parse_fragment("docs/roadmap/milestones.md", body);

    let (milestones, violations) = read_index(&document);

    assert_eq!(milestones.len(), 1);
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("no short outcome message"))
    );
}

#[test]
fn rejects_a_milestone_message_without_rendered_words() {
    let body = "# Roadmap\n\n## Milestone index\n\n\
                | Milestone | Short message |\n|:--|:--|\n\
                | M01 - Visible | \u{200b} |\n\n\
                ## Milestone boundaries\n";
    let document = crate::repository::markdown::parse_fragment("docs/roadmap/milestones.md", body);

    let (_, violations) = read_index(&document);

    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("no short outcome message"))
    );
}

#[test]
fn rejects_raw_html_and_image_alt_as_milestone_messages() {
    let body = "# Roadmap\n\n## Milestone index\n\n\
                | Milestone | Short message |\n|:--|:--|\n\
                | M01 - Raw HTML | <span hidden>Hidden outcome.</span> |\n\
                | M02 - Image | ![Image-only outcome.](image.png) |\n\n\
                ## Milestone boundaries\n";
    let document = crate::repository::markdown::parse_fragment("docs/roadmap/milestones.md", body);

    let (milestones, violations) = read_index(&document);

    assert_eq!(milestones.len(), 2);
    assert!(
        violations
            .iter()
            .any(|violation| { violation.message.contains("milestone index uses raw HTML") })
    );
    assert!(violations.iter().any(|violation| {
        violation
            .message
            .contains("M02 - Image has no short outcome message")
    }));
}

#[test]
fn rejects_raw_html_in_milestone_container_headings() {
    let body = "# Roadmap\n\n## <span hidden>Milestone index</span>\n\n\
                | Milestone | Short message |\n|:--|:--|\n\
                | M01 - First | First outcome. |\n\n\
                ## <span hidden>Milestone boundaries</span>\n\n\
                ### M01 - First\n";
    let document = crate::repository::markdown::parse_fragment("docs/roadmap/milestones.md", body);

    let (_, index_violations) = read_index(&document);
    let (_, boundary_violations) = read_boundaries(&document);

    assert!(
        index_violations
            .iter()
            .any(|violation| { violation.message.contains("milestone index uses raw HTML") })
    );
    assert!(boundary_violations.iter().any(|violation| {
        violation
            .message
            .contains("milestone boundaries use raw HTML")
    }));
}

#[test]
fn rejects_duplicate_milestone_container_sections() {
    let body = "# Roadmap\n\n## Milestone index\n\n\
                | Milestone | Short message |\n|:--|:--|\n\
                | M01 - First | First outcome. |\n\n\
                ## Milestone index\n\nDuplicate.\n\n\
                ## Milestone boundaries\n\n### M01 - First\n\nFirst outcome.\n\n\
                ## Milestone boundaries\n\nDuplicate.\n";
    let document = crate::repository::markdown::parse_fragment("docs/roadmap/milestones.md", body);

    let (_, index_violations) = read_index(&document);
    let (_, boundary_violations) = read_boundaries(&document);

    assert!(index_violations.iter().any(|violation| {
        violation
            .message
            .contains("exactly one rendered `Milestone index`")
    }));
    assert!(boundary_violations.iter().any(|violation| {
        violation
            .message
            .contains("exactly one rendered `Milestone boundaries`")
    }));
}

#[test]
fn rejects_deleting_the_published_milestone_tail() {
    let milestones: Vec<_> = (1..LATEST_PUBLISHED_MILESTONE)
        .map(|number| Milestone {
            number,
            title: format!("M{number:02} - Outcome"),
            line: number,
        })
        .collect();

    let violations = validate_sequence(&milestones);

    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("records M09"))
    );
}

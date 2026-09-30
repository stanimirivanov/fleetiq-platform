use super::{
    CONTRIBUTING, ISSUE_TEMPLATE, Repository, Violation, count_visible_prefixed_lines,
    document_second_level_headings, fenced_block_after_heading, first_second_level,
    has_canonical_milestone_field, has_required_issue_front_matter, heading_line, issue_violation,
    normalize_markdown, template_body, visible_section_body_lines,
};

const ISSUE_HEADINGS: [&str; 5] = [
    "Goal",
    "Scope",
    "Design decisions",
    "Acceptance criteria",
    "Out of scope",
];
const ISSUE_SECTION_BODIES: [(&str, &[&str]); 5] = [
    ("Goal", &["Describe the problem and observable result."]),
    ("Scope", &["- Included behavior and boundaries."]),
    (
        "Design decisions",
        &["- Important choices, assumptions, compatibility effects, and ADR links."],
    ),
    (
        "Acceptance criteria",
        &[
            "- [ ] Observable behavior and verification evidence.",
            "- [ ] Relevant negative and failure behavior.",
            "- [ ] Documentation and operational effects.",
        ],
    ),
    (
        "Out of scope",
        &["- Explicit exclusions and deferred work."],
    ),
];

pub(super) fn check(repository: &Repository) -> Vec<Violation> {
    let Some(policy) = repository.document(CONTRIBUTING) else {
        return vec![issue_violation(
            CONTRIBUTING,
            1,
            "canonical contribution policy is missing",
            "restore the contributor guide and its fenced canonical issue body",
        )];
    };
    let Some(template) = repository.document(ISSUE_TEMPLATE) else {
        return vec![issue_violation(
            ISSUE_TEMPLATE,
            1,
            "implementation issue template is missing",
            "restore the checked-in implementation issue template",
        )];
    };
    let Some((canonical_body, canonical_line)) =
        fenced_block_after_heading(policy, "Issues and milestones", "markdown")
    else {
        return vec![issue_violation(
            CONTRIBUTING,
            heading_line(policy.body(), "Issues and milestones").unwrap_or(1),
            "contributor guide has no fenced canonical Markdown issue body",
            "restore the `markdown` block below `## Issues and milestones`",
        )];
    };

    let canonical_document =
        crate::repository::markdown::parse_fragment(CONTRIBUTING, &canonical_body);
    let actual_body = template_body(template.body());
    let actual_document = crate::repository::markdown::parse_fragment(ISSUE_TEMPLATE, actual_body);
    let mut violations = raw_html_violations(&canonical_document, template, canonical_line);
    if !has_required_issue_front_matter(template.body()) {
        violations.push(issue_violation(
            ISSUE_TEMPLATE,
            1,
            "issue template must begin with the canonical GitHub YAML front matter",
            "restore the canonical name, about, title, labels, and assignees metadata",
        ));
    }
    violations.extend(heading_violations(
        &canonical_document,
        &actual_document,
        template.body(),
        canonical_line,
    ));
    violations.extend(milestone_violations(&canonical_document, &actual_document));
    violations.extend(section_body_violations(
        &canonical_document,
        &actual_document,
        canonical_line,
    ));

    if normalize_markdown(&canonical_body) != normalize_markdown(actual_body) {
        violations.push(issue_violation(
            ISSUE_TEMPLATE,
            1,
            "issue template body differs from the canonical contributor-policy body",
            "copy the complete fenced Markdown issue body from `CONTRIBUTING.md` after the template front matter",
        ));
    }

    violations
}

fn heading_violations(
    canonical: &crate::repository::markdown::Document,
    template: &crate::repository::markdown::Document,
    template_body: &str,
    canonical_line: usize,
) -> Vec<Violation> {
    let required: Vec<_> = ISSUE_HEADINGS.iter().map(ToString::to_string).collect();
    let candidates = [
        (
            CONTRIBUTING,
            canonical_line,
            "canonical issue",
            document_second_level_headings(canonical),
        ),
        (
            ISSUE_TEMPLATE,
            first_second_level(template_body),
            "issue",
            document_second_level_headings(template),
        ),
    ];
    candidates
        .into_iter()
        .filter(|(_, _, _, actual)| actual != &required)
        .map(|(path, line, label, actual)| {
            issue_violation(
                path,
                line,
                format!(
                    "{label} headings `{}` do not match required headings `{}`",
                    actual.join(", "),
                    required.join(", ")
                ),
                "restore Goal, Scope, Design decisions, Acceptance criteria, and Out of scope in that order",
            )
        })
        .collect()
}

fn milestone_violations(
    canonical: &crate::repository::markdown::Document,
    template: &crate::repository::markdown::Document,
) -> Vec<Violation> {
    let canonical_count = count_visible_prefixed_lines(canonical, "Milestone:");
    let template_count = count_visible_prefixed_lines(template, "Milestone:");
    if canonical_count == 1
        && template_count == canonical_count
        && has_canonical_milestone_field(canonical)
        && has_canonical_milestone_field(template)
    {
        return Vec::new();
    }
    vec![issue_violation(
        ISSUE_TEMPLATE,
        1,
        "issue template milestone field has drifted from the canonical issue body",
        "keep exactly one `**Milestone:** MNN - Outcome` field in both sources",
    )]
}

fn section_body_violations(
    canonical: &crate::repository::markdown::Document,
    template: &crate::repository::markdown::Document,
    canonical_line: usize,
) -> Vec<Violation> {
    let candidates = [
        (
            CONTRIBUTING,
            canonical_line,
            "canonical issue body",
            canonical,
        ),
        (ISSUE_TEMPLATE, 1, "issue template", template),
    ];
    candidates
        .into_iter()
        .flat_map(|(path, line, label, document)| {
            ISSUE_SECTION_BODIES
                .iter()
                .filter_map(move |(heading, required)| {
                    let actual = visible_section_body_lines(document, heading);
                    let required: Vec<_> = required.iter().map(ToString::to_string).collect();
                    (actual != required).then(|| issue_violation(
                    path,
                    line,
                    format!(
                            "{label} section `{heading}` does not contain its canonical author prompt"
                    ),
                        "restore the exact visible prompt lines from the enforced issue contract",
                    ))
                })
        })
        .collect()
}

fn raw_html_violations(
    canonical: &crate::repository::markdown::Document,
    template: &crate::repository::markdown::Document,
    canonical_line: usize,
) -> Vec<Violation> {
    let mut violations = Vec::new();
    if let Some(line) = canonical.first_raw_html_between(0, canonical.body().lines().count() + 1) {
        violations.push(issue_violation(
            CONTRIBUTING,
            canonical_line + line - 1,
            "canonical issue body uses raw HTML around policy-bearing content",
            "express every required issue field with plain Markdown",
        ));
    }
    if let Some(line) = template.first_raw_html_between(0, template.body().lines().count() + 1) {
        violations.push(issue_violation(
            ISSUE_TEMPLATE,
            line,
            "issue template uses raw HTML around policy-bearing content",
            "express every required issue field with plain Markdown",
        ));
    }
    violations
}

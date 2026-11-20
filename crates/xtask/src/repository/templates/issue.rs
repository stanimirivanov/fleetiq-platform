use super::{
    ISSUE_TEMPLATE, Repository, Violation, document_second_level_headings,
    has_required_issue_front_matter, issue_violation, visible_section_body_lines,
};

pub(super) fn check(repository: &Repository) -> Vec<Violation> {
    let Some(template) = repository.document(ISSUE_TEMPLATE) else {
        return vec![issue_violation(
            ISSUE_TEMPLATE,
            1,
            "implementation issue template is missing",
            "restore the skill-compatible implementation template",
        )];
    };
    let mut violations = Vec::new();
    if !has_required_issue_front_matter(template.body()) {
        violations.push(issue_violation(
            ISSUE_TEMPLATE,
            1,
            "issue template metadata is invalid",
            "restore the GitHub template name, description and empty defaults",
        ));
    }
    if let Some(line) = template.first_raw_html_between(0, template.body().lines().count() + 1) {
        violations.push(issue_violation(
            ISSUE_TEMPLATE,
            line,
            "issue template uses raw HTML around required content",
            "use visible Markdown for required fields",
        ));
    }
    let headings = document_second_level_headings(template);
    for heading in [
        "Goal",
        "Scope",
        "Dependencies",
        "Acceptance criteria",
        "Verification",
    ] {
        if !headings.iter().any(|actual| actual == heading)
            || visible_section_body_lines(template, heading).is_empty()
        {
            violations.push(issue_violation(
                ISSUE_TEMPLATE, 1, format!("issue template is missing substantive {heading}"),
                "include the implementation boundary, blockers, acceptance and verification prompts",
            ));
        }
    }
    violations
}

use super::{
    CONTRIBUTING, Repository, Violation, fenced_commands_under_heading, first_second_level,
    heading_line, pull_request_policy_violation, pull_request_violation,
    visible_section_cargo_commands, visible_section_contains,
};

const PULL_REQUEST_HEADINGS: [&str; 3] = ["Summary", "Evidence", "Merge Danger"];

const PULL_REQUEST_MARKERS: [(&str, &str); 3] = [
    (
        "Evidence",
        "Checks not run, blocking conditions, and residual risk:",
    ),
    ("Merge Danger", "Door:"),
    ("Merge Danger", "Blast Radius:"),
];

pub(super) fn check(repository: &Repository) -> Vec<Violation> {
    let Some(template) = repository.document(super::PULL_REQUEST_TEMPLATE) else {
        return vec![pull_request_violation(
            1,
            "pull request template is missing",
            "restore the checked-in review template",
        )];
    };
    let actual_headings: Vec<_> = template
        .headings()
        .iter()
        .filter(|heading| heading.level == 2)
        .map(|heading| heading.text.trim().to_owned())
        .collect();
    let expected_headings: Vec<_> = PULL_REQUEST_HEADINGS
        .iter()
        .map(ToString::to_string)
        .collect();
    let mut violations = Vec::new();
    if let Some(line) = template.first_raw_html_between(0, template.body().lines().count() + 1) {
        violations.push(pull_request_violation(
            line,
            "pull request template uses raw HTML around policy-bearing content",
            "express every required prompt and verification row with plain Markdown",
        ));
    }
    if actual_headings != expected_headings {
        violations.push(pull_request_violation(
            first_second_level(template.body()),
            "pull request section names or order have drifted from the review contract",
            "restore Summary, Evidence and Merge Danger from the configured PR skill",
        ));
    }

    for (section, marker) in PULL_REQUEST_MARKERS {
        if !visible_section_contains(template, section, marker) {
            violations.push(pull_request_violation(
                1,
                format!(
                    "pull request template section `{section}` is missing required marker `{marker}`"
                ),
                "restore the prompt in its canonical review section",
            ));
        }
    }

    if let Some(policy) = repository.document(CONTRIBUTING) {
        let required_commands: Vec<_> = crate::verification::Group::Acceptance
            .checks()
            .map(|check| check.command.to_owned())
            .collect();
        let canonical_commands = fenced_commands_under_heading(policy, "Verification");
        if canonical_commands != required_commands {
            violations.push(pull_request_policy_violation(
                CONTRIBUTING,
                heading_line(policy.body(), "Verification").unwrap_or(1),
                "canonical contributor verification commands have drifted from the enforced quality gate",
                "restore the repository, architecture, formatting, SQLx, Clippy, test, and documentation commands in canonical order",
            ));
        }
        let template_commands = visible_section_cargo_commands(template, "Evidence");
        if template_commands != required_commands {
            violations.push(pull_request_violation(
                heading_line(template.body(), "Evidence").unwrap_or(1),
                "verification table commands have drifted from the enforced quality gate",
                "restore every canonical command as an exact first-column table cell in canonical order",
            ));
        }
    }
    violations
}

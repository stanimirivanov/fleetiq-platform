use super::{
    CONTRIBUTING, Repository, Violation, fenced_commands_under_heading, first_second_level,
    heading_line, pull_request_policy_violation, pull_request_violation,
    visible_section_cargo_commands, visible_section_checkbox_lines, visible_section_contains,
};

const PULL_REQUEST_HEADINGS: [&str; 9] = [
    "Issue and milestone",
    "Problem and resulting behavior",
    "Scope and exclusions",
    "Design and compatibility decisions",
    "Architecture",
    "Verification",
    "Migration, rollout, and rollback",
    "Security and operations",
    "Review checklist",
];

const PULL_REQUEST_MARKERS: [(&str, &str); 13] = [
    ("Issue and milestone", "Closes #"),
    ("Issue and milestone", "Milestone:"),
    ("Design and compatibility decisions", "ADRs:"),
    ("Design and compatibility decisions", "Assumptions:"),
    ("Design and compatibility decisions", "Known limitations:"),
    (
        "Design and compatibility decisions",
        "Dependency review (when dependencies change):",
    ),
    ("Architecture", "Owning capability:"),
    ("Architecture", "Inbound adapter or caller:"),
    ("Architecture", "Consumed ports:"),
    ("Architecture", "Outbound adapters:"),
    (
        "Architecture",
        "Dependency edges added, removed, or excepted:",
    ),
    ("Architecture", "Runnable end-to-end behavior:"),
    (
        "Verification",
        "Checks not run, blocking conditions, and residual risk:",
    ),
];

const REQUIRED_VERIFICATION_COMMANDS: [&str; 9] = [
    "cargo xtask repository",
    "cargo xtask architecture",
    "cargo deny check",
    "cargo machete --with-metadata --skip-target-dir",
    "cargo fmt --all --check",
    "cargo sqlx prepare --check --workspace -- --all-targets --all-features",
    "cargo clippy --workspace --all-targets --all-features -- -D warnings",
    "cargo test --workspace --all-features --locked",
    "cargo doc --workspace --no-deps --locked",
];

const PULL_REQUEST_CHECKLIST: [&str; 9] = [
    "- [ ] One coherent capability; unrelated changes are excluded.",
    "- [ ] Repository policy and architecture checks pass.",
    "- [ ] Capability ownership and dependency changes are explicit and inward.",
    "- [ ] Public and persisted contracts have a compatibility plan.",
    "- [ ] Success and relevant invalid and failure paths are verified.",
    "- [ ] Tenant, concurrency, idempotency, retry, and resource risks are covered.",
    "- [ ] Documentation and ADRs are current.",
    "- [ ] Migrations pass empty-database and supported-upgrade checks when relevant.",
    "- [ ] No secrets, customer data, generated noise, or hidden skipped checks.",
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
            "restore the canonical issue, behavior, scope, design, architecture, verification, rollout, security, and review sections",
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
    violations.extend(review_checklist_violations(template));

    if let Some(policy) = repository.document(CONTRIBUTING) {
        let required_commands: Vec<_> = REQUIRED_VERIFICATION_COMMANDS
            .iter()
            .map(ToString::to_string)
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
        let template_commands = visible_section_cargo_commands(template, "Verification");
        if template_commands != required_commands {
            violations.push(pull_request_violation(
                heading_line(template.body(), "Verification").unwrap_or(1),
                "verification table commands have drifted from the enforced quality gate",
                "restore every canonical command as an exact first-column table cell in canonical order",
            ));
        }
    }
    violations
}

fn review_checklist_violations(template: &crate::repository::markdown::Document) -> Vec<Violation> {
    let expected: Vec<_> = PULL_REQUEST_CHECKLIST
        .iter()
        .map(ToString::to_string)
        .collect();
    let actual = visible_section_checkbox_lines(template, "Review checklist");
    if actual == expected {
        return Vec::new();
    }
    vec![pull_request_violation(
        heading_line(template.body(), "Review checklist").unwrap_or(1),
        "review checklist has drifted from the enforced review contract",
        "restore the nine canonical unchecked review prompts in their documented order",
    )]
}

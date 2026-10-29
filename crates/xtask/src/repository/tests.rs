use super::*;
use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};

static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);

#[test]
fn current_repository_satisfies_policy() {
    check_at(Path::new(WORKSPACE_ROOT)).expect("checked-in repository policy should pass");
}

#[test]
fn violation_diagnostics_identify_location_rule_fix_and_policy() {
    let violation = Violation::new(
        "docs/example.md",
        7,
        "example.rule",
        "example violation",
        "correct the example",
        "CONTRIBUTING.md#example",
    );

    assert_eq!(
        violation.to_string(),
        "docs/example.md:7: [example.rule] example violation; fix: correct the example; \
         policy: CONTRIBUTING.md#example"
    );
}

#[test]
fn adr_drift_reports_sequence_and_overlapping_index_coverage() {
    let fixture = Fixture::new();
    fixture.write("docs/adr/0001-first.md", &legacy_adr(1, "First decision"));
    fixture.write("docs/adr/0003-third.md", &legacy_adr(3, "Third decision"));
    fixture.write(
        "docs/adr/README.md",
        "# Decisions\n\n## Index\n\n| Range | Decisions |\n|:--|:--|\n\
         | [0001–0002](0001-first.md) | First group |\n\
         | [0002–0003](0003-third.md) | Second group |\n",
    );
    let repository = markdown::load(&fixture.root).expect("fixture should load");

    let violations = adr::check(&repository);

    assert!(violations.iter().any(|item| item.rule == "adr.sequence"));
    assert!(violations.iter().any(|item| {
        item.rule == "adr.index" && item.message.contains("more than one index row")
    }));
}

#[test]
fn milestone_drift_reports_noncontiguous_index_and_missing_boundary() {
    let fixture = Fixture::new();
    fixture.write(
        "docs/roadmap/milestones.md",
        "# Milestones\n\n## Milestone index\n\n| Milestone | Short message |\n\
         |:--|:--|\n| M01 - First | First. |\n| M03 - Third | Third. |\n\n\
         ## Milestone boundaries\n\n### M01 - First\n\nFirst.\n",
    );
    let repository = markdown::load(&fixture.root).expect("fixture should load");

    let violations = milestones::check(&repository);

    assert!(violations.iter().any(|item| {
        item.rule == "milestone.sequence" && item.message.contains("expected M02")
    }));
    assert!(violations.iter().any(|item| {
        item.rule == "milestone.boundaries" && item.message.contains("no boundary section")
    }));
}

#[test]
fn template_drift_reports_issue_and_pull_request_structure() {
    let fixture = Fixture::new();
    fixture.write(
        "CONTRIBUTING.md",
        "# Contributing\n\n## Issues and milestones\n\n```markdown\n\
         **Milestone:** MNN - Outcome\n\n## Goal\n\nGoal.\n\n\
         ## Acceptance criteria\n\n- [ ] Result.\n```\n\n## Verification\n\n\
         ```bash\ncargo xtask repository\n```\n",
    );
    fixture.write(
        ".github/ISSUE_TEMPLATE/implementation.md",
        "**Milestone:** MNN - Outcome\n\n## Wrong\n\n- [ ] One.\n- [ ] Two.\n",
    );
    fixture.write(
        ".github/PULL_REQUEST_TEMPLATE.md",
        "## Issue and milestone\n\nMilestone: MNN - Outcome\n",
    );
    let repository = markdown::load(&fixture.root).expect("fixture should load");

    let violations = templates::check(&repository);

    assert!(violations.iter().any(|item| item.rule == "template.issue"));
    assert!(
        violations
            .iter()
            .any(|item| item.rule == "template.pull-request")
    );
}

fn legacy_adr(number: usize, title: &str) -> String {
    format!(
        "# ADR {number:04}: {title}\n\n- Status: accepted\n- Date: 2026-09-29\n\n\
         ## Context\n\nContext.\n\n## Decision\n\nDecision.\n\n\
         ## Consequences\n\nConsequences.\n\n\
         ## Alternatives considered\n\nAlternatives.\n"
    )
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "fleetiq-repository-check-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("fixture root should be created");
        Self { root }
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().expect("fixture file should have a parent"))
            .expect("fixture directory should be created");
        fs::write(path, contents).expect("fixture document should be written");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("fixture should be removed");
    }
}

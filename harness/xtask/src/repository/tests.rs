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

#[test]
fn skill_templates_preserve_evidence_and_reject_hidden_requirements() {
    let fixture = Fixture::new();
    let policy = include_str!("../../../../CONTRIBUTING.md");
    let issue = include_str!("../../../../.github/ISSUE_TEMPLATE/implementation.md");
    let pr = include_str!("../../../../.github/PULL_REQUEST_TEMPLATE.md");
    fixture.write("CONTRIBUTING.md", policy);
    fixture.write(".github/ISSUE_TEMPLATE/implementation.md", issue);
    fixture.write(".github/PULL_REQUEST_TEMPLATE.md", pr);
    let check = || templates::check(&markdown::load(&fixture.root).expect("fixture loads"));
    assert_eq!(check(), []);
    fixture.write(
        ".github/ISSUE_TEMPLATE/implementation.md",
        &issue.replace("## Dependencies", "<!-- ## Dependencies -->"),
    );
    assert!(check().iter().any(|item| item.rule == "template.issue"));
    fixture.write(".github/ISSUE_TEMPLATE/implementation.md", issue);
    fixture.write(
        ".github/PULL_REQUEST_TEMPLATE.md",
        &pr.replace(
            "`cargo xtask secrets --base origin/main`",
            "`cargo xtask secrets`",
        ),
    );
    assert!(
        check()
            .iter()
            .any(|item| item.message.contains("verification table"))
    );
    fixture.write(
        ".github/PULL_REQUEST_TEMPLATE.md",
        &pr.replace(
            "**Blast Radius:** affected scope",
            "<!-- Blast Radius: hidden -->",
        ),
    );
    assert!(
        check()
            .iter()
            .any(|item| item.message.contains("Blast Radius"))
    );
}

#[test]
fn repository_policy_requires_the_harness_registry() {
    let fixture = Fixture::new();
    assert!(has_registry_violation(&fixture, "harness.manifest"));
}

#[test]
fn repository_policy_rejects_registry_drift() {
    let fixture = Fixture::new();
    fixture.write("guide.md", "# Guide\n\n## Fix the check\n");
    let mut manifest = serde_json::json!({
        "schema_version": 1,
        "owner": "fleetiq-platform",
        "guides": [{"id": "guide", "documentation": "guide.md#fix-the-check"}],
        "sensors": crate::verification::catalogue::CHECKS.iter().map(|check| {
            serde_json::json!({
                "id": check.id,
                "paired_guide": "guide",
                "remediation": "guide.md#fix-the-check",
                "blocking": true
            })
        }).collect::<Vec<_>>()
    });
    fixture.write("harness/manifest.yaml", &manifest.to_string());
    assert!(!has_registry_violation(&fixture, "harness.registry"));

    let sensors = manifest["sensors"].as_array_mut().unwrap();
    sensors[0]["remediation"] = serde_json::Value::Null;
    sensors[1]["paired_guide"] = "absent".into();
    sensors[2]["id"] = "unregistered".into();
    sensors[3]["remediation"] = "guide.md#nonexistent".into();
    sensors[4]["blocking"] = false.into();
    sensors.push(sensors[0].clone());
    fixture.write("harness/manifest.yaml", &manifest.to_string());

    let Err(RepositoryCheckError::Policy(violations)) = check_at(&fixture.root) else {
        panic!("invalid manifest must fail repository policy");
    };
    for expected in [
        "remediation must name an existing Markdown heading",
        "unknown paired guide",
        "unknown sensor ID",
        "duplicate sensor ID",
        "missing sensor",
        "must be marked blocking",
    ] {
        assert!(
            violations
                .iter()
                .any(|violation| violation.rule == "harness.registry"
                    && violation.message.contains(expected)),
            "missing diagnostic: {expected}; got {violations:?}"
        );
    }
}

fn has_registry_violation(fixture: &Fixture, rule: &str) -> bool {
    matches!(
        check_at(&fixture.root),
        Err(RepositoryCheckError::Policy(violations))
            if violations.iter().any(|violation| violation.rule == rule)
    )
}

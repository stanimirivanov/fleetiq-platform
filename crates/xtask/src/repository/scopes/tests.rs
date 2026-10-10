use std::{collections::BTreeMap, path::PathBuf};

use super::*;
use crate::repository::markdown;

fn valid_repository() -> Repository {
    let documents = [
        (
            "crates/xtask/AGENTS.md",
            "# Sensor work\n\n[Workflow](../../CONTRIBUTING.md)\n\
             [Rust](../../docs/development/engineering-standards.md)\n\
             [Harness](../../docs/development/harness.md)\n\
             [Protection](../../docs/development/harness-protection.md)\n\
             [Verification](../../docs/development/verification.md)\n\
             [Dependencies](../../docs/architecture/dependency-rules.md)\n",
        ),
        (
            "contracts/http/AGENTS.md",
            "# Contract work\n\n[Workflow](../../CONTRIBUTING.md)\n\
             [HTTP contract](README.md)\n\
             [Maintenance](../../.agents/skills/openapi/SKILL.md)\n\
             [Versioning](../../docs/adr/0048-establish-versioned-http-api-boundary.md)\n\
             [Verification](../../docs/development/verification.md)\n",
        ),
        (
            "docs/README.md",
            "# Documentation map\n\n[Sensor work](../crates/xtask/AGENTS.md)\n\
             [Contract work](../contracts/http/AGENTS.md)\n",
        ),
    ]
    .into_iter()
    .map(|(path, body)| (path.to_owned(), markdown::parse_fragment(path, body)))
    .collect::<BTreeMap<_, _>>();
    Repository {
        root: PathBuf::new(),
        documents,
    }
}

fn replace(repository: &mut Repository, source: &str, old: &str, new: &str) {
    let body = repository
        .document(source)
        .unwrap()
        .body()
        .replace(old, new);
    repository
        .documents
        .insert(source.to_owned(), markdown::parse_fragment(source, &body));
}

#[test]
fn accepts_plain_markdown_routes_with_independent_guide_text() {
    let mut repository = valid_repository();
    replace(
        &mut repository,
        "crates/xtask/AGENTS.md",
        "# Sensor work",
        "# A different concise title\n\n<!-- Author-only review notes. -->\n\nAny useful guidance text may change.",
    );
    assert!(check(&repository).is_empty());
}

#[test]
fn accepts_used_reference_links_and_relative_path_normalization() {
    let mut repository = valid_repository();
    replace(
        &mut repository,
        "contracts/http/AGENTS.md",
        "[HTTP contract](README.md)",
        "[HTTP contract][contract]\n\n[contract]: ./README.md#tldr\n",
    );
    replace(
        &mut repository,
        "docs/README.md",
        "../crates/xtask/AGENTS.md",
        "../crates/xtask/./AGENTS.md",
    );
    assert!(check(&repository).is_empty());
}

#[test]
fn rejects_missing_scoped_guides_and_wrong_case() {
    for path in ["crates/xtask/AGENTS.md", "contracts/http/AGENTS.md"] {
        let mut repository = valid_repository();
        let guide = repository.documents.remove(path).unwrap();
        repository.documents.insert(path.to_lowercase(), guide);
        let violations = check(&repository);
        assert_eq!(violations.len(), 1, "missing exact path {path}");
        assert_eq!(violations[0].path, path);
        assert!(violations[0].message.contains("missing"));
    }
}

#[test]
fn rejects_missing_documentation_map() {
    let mut repository = valid_repository();
    repository.documents.remove(MAP);
    let violations = check(&repository);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].path, MAP);
}

#[test]
fn rejects_routes_hidden_in_comments_code_images_or_unused_references() {
    let visible = "[HTTP contract](README.md)";
    for hidden in [
        "<!-- [HTTP contract](README.md) -->",
        "```markdown\n[HTTP contract](README.md)\n```",
        "\n    [HTTP contract](README.md)\n",
        "`[HTTP contract](README.md)`",
        "![HTTP contract](README.md)",
        "[![Icon](README.md)](README.md)",
        "[contract]: README.md",
        "[](README.md)",
    ] {
        let mut repository = valid_repository();
        replace(&mut repository, "contracts/http/AGENTS.md", visible, hidden);
        let violations = check(&repository);
        assert_eq!(violations.len(), 1, "hidden route {hidden}");
        assert!(violations[0].message.contains("not rendered"));
        assert!(violations[0].message.contains("contracts/http/README.md"));
    }
}

#[test]
fn rejects_non_comment_html_without_a_partial_visibility_interpreter() {
    for replacement in [
        "<span hidden>[HTTP contract](README.md)</span>",
        "<div hidden>\n\n[HTTP contract](README.md)\n\n</div>",
        "<!-- note --><span>[HTTP contract](README.md)</span><!-- note -->",
        "[HTTP contract](README.md)\n\n<div>Other content.</div>",
    ] {
        let mut repository = valid_repository();
        replace(
            &mut repository,
            "contracts/http/AGENTS.md",
            "[HTTP contract](README.md)",
            replacement,
        );
        let violations = check(&repository);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].path, "contracts/http/AGENTS.md");
        assert!(violations[0].line > 1);
        assert!(violations[0].message.contains("non-comment raw HTML"));
    }
}

#[test]
fn requires_each_documentation_map_route_to_be_rendered() {
    let mut repository = valid_repository();
    replace(
        &mut repository,
        MAP,
        "[Sensor work](../crates/xtask/AGENTS.md)",
        "<!-- [Sensor work](../crates/xtask/AGENTS.md) -->",
    );
    let violations = check(&repository);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].path, MAP);
    assert!(violations[0].message.contains("crates/xtask/AGENTS.md"));
}

#[test]
fn rejects_wrong_canonical_target_and_external_or_escaping_routes() {
    for destination in [
        "readme.md",
        "https://example.invalid/README.md",
        "../../../contracts/http/README.md",
        "/contracts/http/README.md",
        "C:/contracts/http/README.md",
        r"..\http\README.md",
    ] {
        let mut repository = valid_repository();
        replace(
            &mut repository,
            "contracts/http/AGENTS.md",
            "[HTTP contract](README.md)",
            &format!("[HTTP contract]({destination})"),
        );
        assert_eq!(check(&repository).len(), 1, "target {destination}");
    }
}

#[test]
fn permits_comments_and_html_examples_inside_code_without_counting_them() {
    let mut repository = valid_repository();
    replace(
        &mut repository,
        "contracts/http/AGENTS.md",
        "# Contract work",
        "# Contract work\n\n<!-- First comment. -->\n<!-- Another. -->\n\n```html\n<div hidden>Example</div>\n```",
    );
    assert!(check(&repository).is_empty());
}

#[test]
fn reports_actionable_scope_location_correction_and_policy() {
    let mut repository = valid_repository();
    replace(
        &mut repository,
        "contracts/http/AGENTS.md",
        "[HTTP contract](README.md)",
        "Contract guidance was removed.",
    );
    let violation = &check(&repository)[0];
    let diagnostic = violation.to_string();
    assert!(diagnostic.contains("contracts/http/AGENTS.md:1: [guidance.routes]"));
    assert!(diagnostic.contains("contracts/http/README.md"));
    assert!(diagnostic.contains("fix: add a visible named Markdown link"));
    assert!(diagnostic.contains("policy: docs/development/harness.md#scoped-guidance"));
}

#[test]
fn accepts_multiline_author_comments_with_lf_and_crlf_in_every_route_source() {
    for source in ["crates/xtask/AGENTS.md", "contracts/http/AGENTS.md", MAP] {
        for newline in ["\n", "\r\n"] {
            let mut repository = valid_repository();
            let body = repository.document(source).unwrap().body().to_owned();
            let with_comment =
                format!("<!--\nAuthor-only review notes.\n-->\n\n{body}").replace('\n', newline);
            repository.documents.insert(
                source.to_owned(),
                markdown::parse_fragment(source, &with_comment),
            );
            assert!(check(&repository).is_empty(), "comment in {source}");
        }
    }
}

#[test]
fn rejects_routes_hidden_in_multiline_comments_without_rejecting_the_comment() {
    for newline in ["\n", "\r\n"] {
        let mut repository = valid_repository();
        replace(
            &mut repository,
            "contracts/http/AGENTS.md",
            "[HTTP contract](README.md)",
            &"\n<!--\n[HTTP contract](README.md)\n-->\n".replace('\n', newline),
        );
        let violations = check(&repository);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("not rendered"));
        assert!(!violations[0].message.contains("raw HTML"));
    }
}

#[test]
fn rejects_html_after_a_closed_multiline_comment() {
    for tail in [
        "--><span hidden>Hidden.</span>",
        "-->\n<div>Other content.</div>",
    ] {
        let mut repository = valid_repository();
        let comment_and_html = format!("<!--\nAuthor notes.\n{tail}\n\n");
        replace(
            &mut repository,
            "contracts/http/AGENTS.md",
            "# Contract work",
            &format!("# Contract work\n\n{comment_and_html}"),
        );
        let violations = check(&repository);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("non-comment raw HTML"));
    }
}

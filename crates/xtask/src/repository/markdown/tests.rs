use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;

static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);

#[test]
fn parses_duplicate_anchors_and_ignores_fenced_code_words() {
    let document = parse(
        "guide.md".to_owned(),
        "# Guide\n\n## Same heading!\nreal words\n\n## Same heading!\n```text\nignored words here\n```\n"
            .to_owned(),
    );

    assert_eq!(document.word_count, 7);
    assert_eq!(document.headings[1].anchor, "same-heading");
    assert_eq!(document.headings[2].anchor, "same-heading-1");
}

#[test]
fn reserves_explicitly_colliding_generated_anchors() {
    let document = parse(
        "guide.md".to_owned(),
        "# Guide\n\n## Foo\n\n## Foo-1\n\n## Foo\n".to_owned(),
    );

    assert_eq!(document.headings[1].anchor, "foo");
    assert_eq!(document.headings[2].anchor, "foo-1");
    assert_eq!(document.headings[3].anchor, "foo-2");
}

#[test]
fn separates_rendered_policy_text_from_image_alt_and_hidden_html() {
    let document = parse(
        "guide.md".to_owned(),
        "# ![Visible heading](image.png)\n\n\
         | Milestone | Message |\n|:--|:--|\n\
         | M01 - Visible | <span>Rendered text</span> |\n\
         | M02 - Hidden | <span hidden>Hidden text</span> |\n\
         | M03 - Image | ![Image-only text](image.png) |\n"
            .to_owned(),
    );

    assert_eq!(document.headings[0].text, "");
    assert_eq!(document.headings[0].anchor, "visible-heading");
    assert!(document.visible_text_contains("Rendered text"));
    assert!(document.visible_text_contains("Hidden text"));
    assert!(!document.visible_text_contains("Image-only text"));
    assert!(document.contains_raw_html());
    assert_eq!(document.table_rows[2].cells[1], "");
}

#[test]
fn validates_inline_reference_and_percent_encoded_anchors() {
    let fixture = Fixture::new();
    fixture.write(
        "README.md",
        "# Home\n\n[inline](docs/guide.md#repeated-heading-1)\n[reference][guide]\n\n[guide]: docs/guide.md#Repeated%20Heading\n",
    );
    fixture.write(
        "docs/guide.md",
        "# Guide\n\n## Repeated heading\n\n## Repeated heading\n",
    );
    let repository = load(fixture.path()).expect("fixture should load");

    let violations = check_links(&repository);

    assert_eq!(violations.len(), 1, "anchors are case-sensitive");
}

#[test]
fn ignores_markdown_like_links_inside_code_and_raw_html() {
    let fixture = Fixture::new();
    fixture.write(
        "README.md",
        "# Home\n\n```markdown\n[missing](absent.md)\n```\n<a href=\"absent.md\">raw</a>\n",
    );
    let repository = load(fixture.path()).expect("fixture should load");

    assert!(check_links(&repository).is_empty());
}

#[test]
fn validates_unused_reference_definitions() {
    let fixture = Fixture::new();
    fixture.write("README.md", "# Home\n\n[unused]: missing.md\n");
    let repository = load(fixture.path()).expect("fixture should load");

    let violations = check_links(&repository);

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].rule, "markdown.local-link");
}

#[test]
fn validates_fragments_for_markdown_targets_outside_the_inventory() {
    let fixture = Fixture::new();
    fixture.write(
        "README.md",
        "# Home\n\n[valid](vendor/guide.md#available)\n[broken](vendor/guide.md#missing)\n",
    );
    fixture.write("vendor/guide.md", "# Guide\n\n## Available\n");
    let repository = load(fixture.path()).expect("fixture should load");

    let violations = check_links(&repository);

    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.contains("#missing"));
}

#[cfg(unix)]
#[test]
fn validates_fragments_through_markdown_file_symlinks() {
    let fixture = Fixture::new();
    fixture.write("README.md", "# Home\n\n[broken](guide-link.md#missing)\n");
    fixture.write("docs/guide.md", "# Guide\n\n## Available\n");
    std::os::unix::fs::symlink("docs/guide.md", fixture.path().join("guide-link.md"))
        .expect("fixture symlink should be created");
    let repository = load(fixture.path()).expect("fixture should load");

    let violations = check_links(&repository);

    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.contains("#missing"));
}

#[test]
fn rejects_incorrect_case_and_repository_escape() {
    let fixture = Fixture::new();
    fixture.write(
        "README.md",
        "# Home\n\n[case](Docs/Guide.md)\n[escape](../outside.md)\n\
         [drive](C:/Source/guide.md)\n[file](file:///tmp/guide.md)\n",
    );
    fixture.write("docs/guide.md", "# Guide\n");
    let repository = load(fixture.path()).expect("fixture should load");

    assert_eq!(check_links(&repository).len(), 4);
}

#[test]
fn tldr_threshold_and_policy_guides_are_enforced_with_exemptions() {
    let fixture = Fixture::new();
    fixture.write("long.md", &format!("# Long\n\n{}\n", "word ".repeat(800)));
    fixture.write("docs/architecture/short.md", "# Architecture\n\nShort.\n");
    fixture.write(
        "docs/architecture/compliant.md",
        "# Architecture\n\n## TL;DR\n\nShort.\n",
    );
    fixture.write(
        "docs/architecture/late-summary.md",
        "# Architecture\n\nBackground before the summary.\n\n## TL;DR\n\nShort.\n",
    );
    fixture.write(
        "docs/architecture/hidden-summary.md",
        "# Architecture\n\n## <span hidden>TL;DR</span>\n\nShort.\n",
    );
    fixture.write(
        "docs/architecture/raw-before-summary.md",
        "# Architecture\n\n<div>Background.</div>\n\n## TL;DR\n\nShort.\n",
    );
    fixture.write(
        "docs/architecture/image-before-summary.md",
        "# Architecture\n\n![Background](image.png)\n\n## TL;DR\n\nShort.\n",
    );
    fixture.write(
        "docs/architecture/code-before-summary.md",
        "# Architecture\n\n```text\nBackground.\n```\n\n## TL;DR\n\nShort.\n",
    );
    fixture.write(
        "docs/architecture/empty-summary.md",
        "# Architecture\n\n## TL;DR\n\n## Context\n\nDetails.\n",
    );
    fixture.write(
        "docs/adr/0001-long.md",
        &format!("# ADR\n\n{}\n", "word ".repeat(800)),
    );
    fixture.write(
        ".github/ISSUE_TEMPLATE.md",
        &format!("# Template\n\n{}\n", "word ".repeat(800)),
    );
    fixture.write(
        ".github/security-guide.md",
        &format!("# Security\n\n{}\n", "word ".repeat(800)),
    );
    fixture.write(
        "docs/development/security-review.md",
        "# Security review\n\nShort.\n",
    );
    let repository = load(fixture.path()).expect("fixture should load");

    assert_eq!(check_tldr(&repository).len(), 10);
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "fleetiq-markdown-check-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("fixture root should be created");
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
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

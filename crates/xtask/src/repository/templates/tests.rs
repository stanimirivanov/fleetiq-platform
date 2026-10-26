use super::*;

#[test]
fn extracts_canonical_issue_body_from_backtick_fence() {
    let body =
        "# Policy\n\n## Issues and milestones\n\n```markdown\n## Goal\n\nText.\n```\n\n## Next\n";
    let policy = crate::repository::markdown::parse_fragment("CONTRIBUTING.md", body);
    let (block, _) = fenced_block_after_heading(&policy, "Issues and milestones", "markdown")
        .expect("fenced block should be found");
    let document = crate::repository::markdown::parse_fragment("CONTRIBUTING.md", &block);
    assert_eq!(document_second_level_headings(&document), vec!["Goal"]);
}

#[test]
fn extracts_cargo_commands_from_every_verification_fence() {
    let body = "# Policy\n\n## Verification\n\n```powershell\ncargo xtask repository\n\
                $env:DATABASE_URL = 'ignored'\n```\nText.\n~~~powershell\n\
                cargo test --workspace\n~~~\n\n## Documentation\n";
    assert_eq!(
        fenced_commands_under_heading(
            &crate::repository::markdown::parse_fragment("CONTRIBUTING.md", body),
            "Verification"
        ),
        vec!["cargo xtask repository", "cargo test --workspace"]
    );
}

#[test]
fn reads_only_visible_lines_from_the_named_issue_section() {
    let document = crate::repository::markdown::parse_fragment(
        ".github/ISSUE_TEMPLATE/implementation.md",
        "## Scope\n\n- [ ] Wrong section\n\n## Acceptance criteria\n\n\
         - [ ] First\n- [x] Second\n- [X] Third\n<!-- - [ ] Hidden -->\n",
    );
    assert_eq!(
        visible_section_body_lines(&document, "Acceptance criteria"),
        vec!["- [ ] First", "- [x] Second", "- [X] Third"]
    );
}

#[test]
fn removes_issue_template_front_matter_before_comparison() {
    let body = "---\nname: Implementation task\n\
                about: Propose one coherent, reviewable capability\n\
                title: \"\"\nlabels: \"\"\nassignees: \"\"\n---\n\n\
                ## Goal\n\nDescribe it.\n";
    assert_eq!(template_body(body), "## Goal\n\nDescribe it.\n");
    assert_eq!(
        normalize_markdown(template_body(body)),
        "## Goal\n\nDescribe it."
    );
    assert!(has_required_issue_front_matter(body));
    assert!(!has_required_issue_front_matter(
        "## Goal\n\nDescribe it.\n"
    ));
    assert!(!has_required_issue_front_matter(
        "---\nname: Implementation task\nabout: null\n---\n\n## Goal\n"
    ));
}

#[test]
fn ignores_pull_request_structure_hidden_in_comments() {
    let body = "<!--\n## Issue and milestone\n\nCloses #\n\n\
                `cargo xtask repository`\n-->\n";
    let document =
        crate::repository::markdown::parse_fragment(".github/PULL_REQUEST_TEMPLATE.md", body);

    assert_eq!(document_second_level_headings(&document).len(), 0);
    assert!(!visible_body_contains(&document, "Closes #"));
    assert!(!visible_body_contains(
        &document,
        "`cargo xtask repository`"
    ));
    assert!(
        !document.contains_raw_html(),
        "comment lines: {:?}",
        document.first_raw_html_between(0, document.body().lines().count() + 1)
    );

    let mixed = crate::repository::markdown::parse_fragment(
        ".github/PULL_REQUEST_TEMPLATE.md",
        "visible <!-- Closes # and cargo xtask repository -->\n",
    );
    assert!(!visible_body_contains(&mixed, "Closes #"));
    assert!(!visible_body_contains(&mixed, "cargo xtask repository"));

    let image = crate::repository::markdown::parse_fragment(
        ".github/PULL_REQUEST_TEMPLATE.md",
        "![Closes # and cargo xtask repository](https://example.invalid/pixel)\n",
    );
    assert!(!visible_body_contains(&image, "Closes #"));
    assert!(!visible_body_contains(&image, "cargo xtask repository"));

    let hidden_html = crate::repository::markdown::parse_fragment(
        ".github/PULL_REQUEST_TEMPLATE.md",
        "visible <span hidden>Closes # and cargo xtask repository</span>\n",
    );
    assert!(visible_body_contains(&hidden_html, "Closes #"));
    assert!(hidden_html.contains_raw_html());

    let mixed_html = crate::repository::markdown::parse_fragment(
        ".github/PULL_REQUEST_TEMPLATE.md",
        "<!-- author note --> <span hidden>Closes #</span><!-- end -->\n",
    );
    assert!(mixed_html.contains_raw_html());
}

#[test]
fn requires_pull_request_prompts_in_their_canonical_sections() {
    let body =
        "## Issue and milestone\n\nCloses #\n\n## Verification\n\nMilestone: MNN - Outcome\n";
    let document =
        crate::repository::markdown::parse_fragment(".github/PULL_REQUEST_TEMPLATE.md", body);

    assert!(visible_section_contains(
        &document,
        "Issue and milestone",
        "Closes #"
    ));
    assert!(!visible_section_contains(
        &document,
        "Issue and milestone",
        "Milestone:"
    ));
    assert!(visible_section_contains(
        &document,
        "Verification",
        "Milestone:"
    ));
}

#[test]
fn requires_verification_commands_as_exact_table_cells() {
    let document = crate::repository::markdown::parse_fragment(
        ".github/PULL_REQUEST_TEMPLATE.md",
        "## Verification\n\n| Command | Result |\n|:--|:--|\n\
         | `cargo test --workspace` | Passed |\n\
         | `cargo fmt --all --check --extra` | Passed |\n\n\
         | ~~`cargo clippy --workspace`~~ | Passed |\n\
         | `cargo doc --workspace` --extra | `cargo doc --workspace` |\n\n\
         Do not run cargo build --workspace.\n",
    );

    assert_eq!(
        visible_section_cargo_commands(&document, "Verification"),
        vec!["cargo test --workspace", "cargo fmt --all --check --extra"]
    );
}

#[test]
fn reads_only_visible_unchecked_items_from_the_review_section() {
    let document = crate::repository::markdown::parse_fragment(
        ".github/PULL_REQUEST_TEMPLATE.md",
        "## Scope and exclusions\n\n- [ ] Wrong section.\n\n\
         ## Review checklist\n\n- [ ] First.\n- [x] Already checked.\n\
         <!-- - [ ] Hidden. -->\n- [ ] Second.\n",
    );

    assert_eq!(
        visible_section_checkbox_lines(&document, "Review checklist"),
        vec!["- [ ] First.", "- [ ] Second."]
    );
}

#[test]
fn identifies_raw_html_in_an_issue_body() {
    let document = crate::repository::markdown::parse_fragment(
        ".github/ISSUE_TEMPLATE/implementation.md",
        "## <span hidden>Goal</span>\n",
    );

    assert!(document.contains_raw_html());
    assert_eq!(document.first_raw_html_between(0, 2), Some(1));
}

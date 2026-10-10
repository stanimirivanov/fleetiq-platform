use std::{fs, path::Path};

use super::{
    configuration::{is_exact_literal, validate},
    detector::safe_findings,
    fixtures::synthetic_token,
    snapshot::{TemporaryDirectory, candidate_paths, git, incoming_range},
};

const DEFAULT: &str = "[extend]\nuseDefault = true\n";

#[test]
fn default_rules_cannot_be_disabled_or_replaced() {
    for policy in [
        "",
        "[extend]\nuseDefault = false",
        "[extend]\nuseDefault = true\ndisabledRules = ['github-pat']",
        "[extend]\nuseDefault = true\n[[rules]]\nid = 'replacement'",
    ] {
        assert!(validate(policy).is_err());
    }
    assert!(validate(DEFAULT).is_ok());
}

#[test]
fn exceptions_require_exact_secret_and_path_and_owned_review_trigger() {
    let valid = exception("AND", "^fixtures/key\\.txt$", "^synthetic-value$");
    assert!(validate(&valid).is_ok());
    for invalid in [
        exception("OR", "^fixtures/key\\.txt$", "^synthetic-value$"),
        exception("AND", ".*", "^synthetic-value$"),
        exception("AND", "^fixtures/key\\.txt$", ".*"),
        valid.replace("Owner: maintainers", "Owner: "),
        valid.replace("Review trigger: upgrade", "Review trigger: "),
    ] {
        assert!(validate(&invalid).is_err());
    }
}

#[test]
fn literal_allowlist_rejects_regex_bypasses() {
    for pattern in [
        "^.*$",
        "^key[0-9]$",
        "^value|other$",
        "^\\d$",
        "^\\Qvalue\\E$",
        "^$",
        "value",
        "^value\\$",
    ] {
        assert!(!is_exact_literal(pattern), "{pattern}");
    }
    assert!(is_exact_literal("^nested/key\\.txt$"));
    assert!(is_exact_literal("^literal\\+value$"));
}

#[test]
fn candidate_selection_is_deduplicated_and_rejects_traversal() {
    let paths = candidate_paths(b"a.txt\0nested/b.txt\0a.txt\0").unwrap();
    assert_eq!(paths.len(), 2);
    for value in [b"../outside\0".as_slice(), b"/outside\0", b"\xff\0"] {
        assert!(candidate_paths(value).is_err());
    }
}

#[test]
fn report_retains_only_safe_location_rule_and_line() {
    let secret = synthetic_token();
    let report = serde_json::json!([{"File": "candidate.txt", "StartLine": 4, "RuleID": "github-pat", "Secret": secret, "Match": secret, "Line": secret, "Description": secret, "Author": secret}]);
    let result = safe_findings(&serde_json::to_vec(&report).unwrap(), Path::new("snapshot"))
        .unwrap()
        .join("\n");
    assert!(result.contains("candidate.txt"));
    assert!(result.contains("github-pat"));
    assert!(result.contains(":4"));
    assert!(!result.contains(&secret));
}

#[test]
fn report_metadata_cannot_escape_input_scope_or_inject_rule_text() {
    for (report, expected) in [
        (
            serde_json::json!([{"File":"../outside", "StartLine":1, "RuleID":"github-pat", "Secret":"synthetic"}]),
            "outside its selected inputs",
        ),
        (
            serde_json::json!([{"File":"file.txt", "StartLine":1, "RuleID":"github-pat\nsecret", "Secret":"synthetic"}]),
            "invalid rule identifier",
        ),
        (
            serde_json::json!([{"File":"file.txt", "StartLine":0, "RuleID":"github-pat", "Secret":"synthetic"}]),
            "positive line number",
        ),
        (serde_json::json!({"File":"file.txt"}), "not an array"),
    ] {
        let error = safe_findings(&serde_json::to_vec(&report).unwrap(), Path::new("snapshot"))
            .unwrap_err();
        assert!(error.to_string().contains(expected));
    }
}
#[test]
fn empty_and_malformed_reports_do_not_invent_a_pass() {
    assert!(
        safe_findings(b"[]", Path::new("snapshot"))
            .unwrap()
            .is_empty()
    );
    assert!(safe_findings(b"not JSON", Path::new("snapshot")).is_err());
    assert!(safe_findings(b"[{}]", Path::new("snapshot")).is_err());
}

#[test]
fn trusted_base_must_resolve_to_an_ancestor_commit() {
    let temporary = TemporaryDirectory::create().unwrap();
    let root = temporary.path();
    git(root, &["init", "--quiet"]).unwrap();
    git(root, &["config", "user.name", "fixture"]).unwrap();
    git(root, &["config", "user.email", "fixture@example.invalid"]).unwrap();
    fs::write(root.join("file.txt"), "safe").unwrap();
    git(root, &["add", "."]).unwrap();
    git(root, &["commit", "--quiet", "-m", "base"]).unwrap();
    assert!(incoming_range(root, "HEAD").is_ok());
    assert!(incoming_range(root, "--all").is_err());
    assert!(incoming_range(root, "missing-branch").is_err());
    let base = super::snapshot::resolve_commit(root, "HEAD").unwrap();
    git(root, &["checkout", "--quiet", "--orphan", "unrelated"]).unwrap();
    git(root, &["commit", "--quiet", "-m", "unrelated"]).unwrap();
    assert!(incoming_range(root, &base).is_err());
}

fn exception(condition: &str, path: &str, secret: &str) -> String {
    format!(
        "{DEFAULT}[[allowlists]]\ndescription = 'Owner: maintainers; Reason: public fixture; Review trigger: upgrade'\ncondition = '{condition}'\nregexTarget = 'secret'\npaths = ['{path}']\nregexes = ['{secret}']\n"
    )
}

#[test]
fn raw_index_protocol_rejects_unmerged_links_ignores_and_traversal() {
    let hash = "a".repeat(40);
    let valid = format!("100644 {hash} 0\tfile.txt\0");
    let entries = super::index::parse_entries(valid.as_bytes()).unwrap();
    assert_eq!(entries.len(), 1);
    for record in [
        format!("100644 {hash} 1\tfile.txt\0"),
        format!("120000 {hash} 0\tfile.txt\0"),
        format!("160000 {hash} 0\tsubmodule\0"),
        format!("100644 {hash} 0\t../outside\0"),
        format!("100644 {hash} 0\t.gitleaksignore\0"),
        "100644 invalid 0\tfile.txt\0".to_owned(),
    ] {
        assert!(super::index::parse_entries(record.as_bytes()).is_err());
    }
    let blob = format!("{hash} blob 4\nsafe\n");
    assert_eq!(
        super::index::parse_blobs(&entries, blob.as_bytes()).unwrap()[Path::new("file.txt")],
        b"safe"
    );
    for malformed in [
        format!("{hash} blob 5\nsafe\n"),
        format!("{hash} commit 4\nsafe\n"),
        format!("{} blob 4\nsafe\n", "b".repeat(40)),
        format!("{blob}extra"),
    ] {
        assert!(super::index::parse_blobs(&entries, malformed.as_bytes()).is_err());
    }
}

#[test]
fn portable_paths_remain_exact_and_escaped_punctuation_remains_literal() {
    let valid = exception("AND", r"^fixtures[/\\]key\.txt$", r"^ghp\_synthetic$");
    assert!(validate(&valid).is_ok());
    assert!(validate(&valid.replace(r"[/\\]", ".*")).is_err());
    assert!(is_exact_literal(r"^\-header\-$"));
    assert!(is_exact_literal(r"^escaped\_literal$"));
}

#[test]
fn replaced_ancestor_link_is_rejected_before_external_bytes_are_copied() {
    let temporary = TemporaryDirectory::create().unwrap();
    let root = temporary.path().join("repository");
    let outside = temporary.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    git(&root, &["init", "--quiet"]).unwrap();
    let folder = root.join("nested");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("file.txt"), "tracked safe fixture").unwrap();
    git(&root, &["add", "."]).unwrap();
    fs::remove_file(folder.join("file.txt")).unwrap();
    fs::remove_dir(&folder).unwrap();
    fs::write(outside.join("file.txt"), "outside bytes must remain unread").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &folder).unwrap();
    #[cfg(windows)]
    {
        // Junction creation needs no Windows symlink privilege or developer mode.
        let status = std::process::Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&folder)
            .arg(&outside)
            .output()
            .unwrap()
            .status;
        assert!(status.success(), "fixture junction creation must succeed");
    }
    let snapshot = temporary.path().join("snapshot");
    let error = super::snapshot::copy_candidates(&root, &snapshot).unwrap_err();
    assert!(
        error.to_string().contains("directory links") || error.to_string().contains("symlinks")
    );
    assert!(!snapshot.join("nested/file.txt").exists());
    assert_eq!(
        fs::read_to_string(outside.join("file.txt")).unwrap(),
        "outside bytes must remain unread"
    );
}

#[test]
fn exact_wrapper_exceptions_never_suppress_another_secret_or_another_file() {
    let source = exception("AND", r"^fixtures[/\\]key\.txt$", "^public-synthetic$");
    let exceptions = super::configuration::exceptions(&source).unwrap();
    for (path, secret, expected) in [
        ("fixtures/key.txt", "public-synthetic", 0),
        ("fixtures/key.txt", "another-secret", 1),
        ("fixtures/other.txt", "public-synthetic", 1),
    ] {
        let report = serde_json::json!([{"File": path, "StartLine": 1, "RuleID": "generic-api-key", "Secret": secret, "Line": secret, "Match": secret}]);
        let (raw, safe) = super::detector::filtered_findings(
            &serde_json::to_vec(&report).unwrap(),
            Path::new("snapshot"),
            &exceptions,
        )
        .unwrap();
        assert_eq!(raw, 1);
        assert_eq!(safe.len(), expected);
        assert!(!safe.join("\n").contains(secret));
    }
}

#[test]
fn fingerprint_ignore_names_are_rejected_case_insensitively_at_every_depth() {
    for path in [
        ".gitleaksignore",
        ".GITLEAKSIGNORE",
        "nested/.GitLeaksIgnore",
    ] {
        assert!(super::snapshot::reject_ignore(Path::new(path)).is_err());
    }
    assert!(super::snapshot::reject_ignore(Path::new("ignore-notes.txt")).is_ok());
}

#[test]
fn missing_private_value_evidence_never_suppresses_a_finding() {
    let exceptions =
        super::configuration::exceptions(&exception("AND", "^file\\.txt$", "^public-value$"))
            .unwrap();
    let report =
        serde_json::json!([{"File":"file.txt", "StartLine":1, "RuleID":"generic-api-key"}]);
    assert!(
        super::detector::filtered_findings(
            &serde_json::to_vec(&report).unwrap(),
            Path::new("snapshot"),
            &exceptions
        )
        .is_err()
    );
    let empty = serde_json::json!([{"File":"file.txt", "StartLine":1, "RuleID":"generic-api-key", "Secret":""}]);
    assert!(
        super::detector::filtered_findings(
            &serde_json::to_vec(&empty).unwrap(),
            Path::new("snapshot"),
            &exceptions
        )
        .is_err()
    );
}

#[test]
fn public_metadata_redacts_secret_values_from_other_findings_as_well() {
    let token = synthetic_token();
    let other = "separate-synthetic-value";
    let report = serde_json::json!([
        {"File":format!("nested/{token}.txt"), "StartLine":1, "RuleID":"generic-api-key", "Secret":other},
        {"File":"second.txt", "StartLine":2, "RuleID":"github-pat", "Secret":token}
    ]);
    let result = safe_findings(&serde_json::to_vec(&report).unwrap(), Path::new("snapshot"))
        .unwrap()
        .join("\n");
    assert!(result.contains("REDACTED"));
    assert!(!result.contains(&token));
    assert!(!result.contains(other));
}

#[cfg(unix)]
#[test]
fn literal_backslash_filename_is_not_an_exact_subdirectory_exception() {
    let exceptions = super::configuration::exceptions(&exception(
        "AND",
        r"^fixtures/key\.txt$",
        "^public-value$",
    ))
    .unwrap();
    let report = serde_json::json!([{"File":r"fixtures\key.txt", "StartLine":1, "RuleID":"generic-api-key", "Secret":"public-value"}]);
    let (_, findings) = super::detector::filtered_findings(
        &serde_json::to_vec(&report).unwrap(),
        Path::new("snapshot"),
        &exceptions,
    )
    .unwrap();
    assert_eq!(findings.len(), 1);
}

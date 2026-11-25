use super::*;

fn reviewed_lockfile(oidc_version: &str, rsa_version: &str, reference: &str) -> String {
    format!(
        r#"version = 4

[[package]]
name = "openidconnect"
version = "{oidc_version}"
source = "registry+https://github.com/rust-lang/crates.io-index"
dependencies = ["{reference}"]

[[package]]
name = "rsa"
version = "{rsa_version}"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "fleetiq-server"
version = "0.1.0"
dependencies = ["openidconnect"]
"#
    )
}

#[test]
fn accepts_only_the_reviewed_graph_with_cargo_reference_qualifiers() {
    for reference in [
        "rsa",
        "rsa 0.9.10",
        "rsa 0.9.10 (registry+https://github.com/rust-lang/crates.io-index)",
    ] {
        validate_lockfile(&reviewed_lockfile("4.0.1", "0.9.10", reference))
            .expect("the reviewed verification-only edge should pass");
    }
}

#[test]
fn rejects_changed_oidc_versions_including_prereleases_and_build_metadata() {
    for version in ["4.0.0", "4.0.2", "4.0.1-beta.1", "4.0.1+unreviewed"] {
        let error = validate_lockfile(&reviewed_lockfile(version, "0.9.10", "rsa"))
            .expect_err("any OIDC version change requires a new security review");
        assert!(error.contains("unreviewed `openidconnect` version"));
        assert!(error.contains(REVIEW));
    }
}

#[test]
fn rejects_changed_rsa_versions_including_prereleases() {
    for version in ["0.9.9", "0.9.11", "0.9.10-beta.1"] {
        let error = validate_lockfile(&reviewed_lockfile("4.0.1", version, "rsa"))
            .expect_err("the exception does not approve another RSA release");
        assert!(error.contains("unreviewed `rsa` version"));
    }
}

#[test]
fn rejects_an_additional_rsa_consumer_even_when_oidc_is_also_present() {
    for reference in [
        "rsa",
        "rsa 0.9.10",
        "rsa 0.9.10 (registry+https://github.com/rust-lang/crates.io-index)",
    ] {
        let lockfile = format!(
            "{}\n[[package]]\nname = \"unreviewed-consumer\"\nversion = \"1.0.0\"\ndependencies = [\"{reference}\"]\n",
            reviewed_lockfile("4.0.1", "0.9.10", "rsa"),
        );
        let error = validate_lockfile(&lockfile).expect_err("every direct RSA consumer is checked");
        assert!(error.contains("unreviewed RSA consumer `unreviewed-consumer`"));
    }
}

#[test]
fn rejects_multiple_reviewed_package_records() {
    for (name, version) in [("openidconnect", "4.0.1"), ("rsa", "0.9.10")] {
        let lockfile = format!(
            "{}\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\nsource = \"{REGISTRY}\"\n",
            reviewed_lockfile("4.0.1", "0.9.10", "rsa"),
        );
        let error = validate_lockfile(&lockfile).expect_err("additional versions are not approved");
        assert!(error.contains(&format!("multiple `{name}` packages")));
    }
}

#[test]
fn rejects_an_unreviewed_source_despite_matching_name_and_version() {
    let lockfile = reviewed_lockfile("4.0.1", "0.9.10", "rsa")
        .replace(REGISTRY, "git+https://example.invalid/unreviewed");
    let error = validate_lockfile(&lockfile).expect_err("source changes require review");
    assert!(error.contains("must come from crates.io"));
}

#[test]
fn rejects_a_stale_exception_or_wrong_dependency_qualifier() {
    for reference in [
        "unrelated",
        "rsa 0.9.11",
        "rsa 0.9.10 (git+https://example.invalid)",
    ] {
        validate_lockfile(&reviewed_lockfile("4.0.1", "0.9.10", reference))
            .expect_err("missing or changed edges cannot inherit the exception");
    }
}

#[test]
fn rejects_malformed_toml_and_malformed_lockfile_shapes() {
    for lockfile in [
        "",
        "version = 4\n[[package]",
        "version = 4",
        "version = 3\npackage = []",
        "version = 4\npackage = [42]",
        "version = 4\n[[package]]\nname = 42\nversion = \"1.0.0\"",
        "version = 4\n[[package]]\nname = \"\"\nversion = \"1.0.0\"",
        "version = 4\n[[package]]\nname = \"other\"\nversion = false",
        "version = 4\n[[package]]\nname = \"other\"\nversion = \"1.0.0\"\nsource = 42",
        "version = 4\n[[package]]\nname = \"other\"\nversion = \"1.0.0\"\ndependencies = false",
        "version = 4\n[[package]]\nname = \"other\"\nversion = \"1.0.0\"\ndependencies = [42]",
    ] {
        let error = validate_lockfile(lockfile).expect_err("malformed locks must fail closed");
        assert!(error.contains("RUSTSEC-2023-0071 exception"));
        assert!(error.contains(REVIEW));
    }
}

#[test]
fn the_current_complete_lockfile_matches_the_approved_exception() {
    let path = Path::new(super::super::WORKSPACE_MANIFEST).with_file_name("Cargo.lock");
    check(&path).expect("the committed complete graph should retain the exact approved scope");
}

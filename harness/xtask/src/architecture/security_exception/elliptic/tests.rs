use std::fmt::Write as _;

use super::{PACKAGES, REGISTRY, validate_lockfile};

// A Cargo-shaped fixture includes unrelated outgoing dependencies. Only incoming
// edges to the reviewed stack are restricted; normal OIDC dependencies remain.
fn fixture() -> toml::Table {
    let records = [
        ("fleetiq-server", "0.1.0", vec!["openidconnect", "tokio"]),
        ("openidconnect", "4.0.1", vec!["p256", "p384", "rsa"]),
        (
            "p256",
            "0.13.2",
            vec!["ecdsa", "elliptic-curve", "primeorder"],
        ),
        (
            "p384",
            "0.13.1",
            vec!["ecdsa", "elliptic-curve", "primeorder"],
        ),
        ("ecdsa", "0.16.9", vec!["elliptic-curve"]),
        ("primeorder", "0.13.6", vec!["elliptic-curve"]),
        ("elliptic-curve", "0.13.8", vec!["crypto-bigint"]),
        ("crypto-bigint", "0.5.5", vec![]),
        ("rsa", "0.9.10", vec![]),
        ("tokio", "1.53.3", vec![]),
    ];
    let mut source = String::from("version = 4\n");
    for (name, version, dependencies) in records {
        writeln!(
            source,
            "\n[[package]]\nname = {name:?}\nversion = {version:?}\ndependencies = {dependencies:?}"
        )
        .expect("write fixture");
        if name != "fleetiq-server" {
            writeln!(source, "source = {REGISTRY:?}").expect("write source");
        }
    }
    source.parse().expect("valid lock fixture")
}

fn package<'a>(document: &'a mut toml::Table, name: &str) -> &'a mut toml::Value {
    document["package"]
        .as_array_mut()
        .expect("packages")
        .iter_mut()
        .find(|record| record["name"].as_str() == Some(name))
        .expect("fixture package")
}

// The workspace intentionally enables TOML parsing only. Keep fixture rendering
// local instead of adding a serializer feature to production tooling.
fn render(document: &toml::Table) -> String {
    fn value(input: &toml::Value) -> String {
        match input {
            toml::Value::String(text) => format!("{text:?}"),
            toml::Value::Integer(number) => number.to_string(),
            toml::Value::Array(items) => format!(
                "[{}]",
                items.iter().map(value).collect::<Vec<_>>().join(", ")
            ),
            _ => panic!("unsupported fixture value"),
        }
    }
    let mut source = String::from("version = 4\n");
    for record in document["package"].as_array().expect("fixture packages") {
        source.push_str("\n[[package]]\n");
        for (key, field) in record.as_table().expect("fixture record") {
            writeln!(source, "{key} = {}", value(field)).expect("write field");
        }
    }
    source
}

fn rejection(document: &toml::Table, diagnostic: &str) {
    let error = validate_lockfile(&render(document)).expect_err("unreviewed graph must fail");
    assert!(error.contains("RUSTSEC-2026-0387"), "{error}");
    assert!(error.contains(diagnostic), "{error}");
}

#[test]
fn accepts_cargo_reference_forms_and_unrelated_outgoing_dependencies() {
    for qualified in [false, true] {
        let mut document = fixture();
        for record in document["package"].as_array_mut().expect("packages") {
            for reference in record["dependencies"].as_array_mut().expect("dependencies") {
                let name = reference.as_str().expect("reference");
                if let Some(&(_, version)) = PACKAGES.iter().find(|&&(target, _)| name == target) {
                    *reference = if qualified {
                        format!("{name} {version} ({REGISTRY})").into()
                    } else {
                        format!("{name} {version}").into()
                    };
                }
            }
        }
        validate_lockfile(&render(&document)).expect("approved qualified graph");
    }
    validate_lockfile(&render(&fixture())).expect("approved bare graph");
}

#[test]
fn rejects_new_consumers_at_every_level_including_the_composition_root() {
    for &(name, _) in PACKAGES {
        let mut document = fixture();
        package(&mut document, "tokio")["dependencies"] = vec![toml::Value::from(name)].into();
        rejection(&document, "unreviewed consumer edge");
    }
    // An indirect new caller is still rejected at the reviewed boundary.
    let mut document = fixture();
    let extra: toml::Table = "name = 'new-service'\nversion = '0.1.0'\ndependencies = ['tokio']"
        .parse()
        .expect("package");
    document["package"]
        .as_array_mut()
        .expect("packages")
        .push(extra.into());
    package(&mut document, "tokio")["dependencies"] = vec![toml::Value::from("p256")].into();
    rejection(&document, "tokio -> p256");
}

#[test]
fn rejects_changed_versions_sources_duplicates_and_missing_nodes() {
    for &(name, version) in PACKAGES {
        for changed in [
            "99.0.0".to_owned(),
            format!("{version}-rc.1"),
            format!("{version}+unreviewed"),
        ] {
            let mut document = fixture();
            package(&mut document, name)["version"] = changed.into();
            rejection(&document, "unreviewed version");
        }
        let mut document = fixture();
        package(&mut document, name)
            .as_table_mut()
            .expect("package table")
            .insert(
                "source".to_owned(),
                "git+https://example.invalid/unreviewed".into(),
            );
        rejection(&document, "unreviewed source");
        let mut document = fixture();
        let duplicate = package(&mut document, name).clone();
        document["package"]
            .as_array_mut()
            .expect("packages")
            .push(duplicate);
        rejection(&document, "duplicate reviewed package");
        let mut document = fixture();
        document["package"]
            .as_array_mut()
            .expect("packages")
            .retain(|record| record["name"].as_str() != Some(name));
        rejection(&document, "is missing");
    }
}

#[test]
fn rejects_missing_duplicate_or_misqualified_edges() {
    for &(consumer, target) in super::EDGES {
        let mut document = fixture();
        package(&mut document, consumer)["dependencies"]
            .as_array_mut()
            .expect("dependencies")
            .retain(|reference| reference.as_str() != Some(target));
        rejection(&document, "required edge");
        for suffix in [
            " 99.0.0",
            " 0.5.5 (git+https://example.invalid)",
            " malformed extra tokens",
        ] {
            let mut document = fixture();
            let references = package(&mut document, consumer)["dependencies"]
                .as_array_mut()
                .expect("dependencies");
            *references
                .iter_mut()
                .find(|reference| reference.as_str() == Some(target))
                .expect("edge") = format!("{target}{suffix}").into();
            rejection(&document, "unreviewed dependency reference");
        }
        let mut document = fixture();
        package(&mut document, consumer)["dependencies"]
            .as_array_mut()
            .expect("dependencies")
            .push(target.into());
        rejection(&document, "duplicate protected dependency edge");
    }
}

#[test]
fn rejects_malformed_lock_data_without_panicking_or_skipping_records() {
    for source in [
        "not toml",
        "version = 3",
        "version = 4",
        "version = 4\npackage = [42]",
    ] {
        assert!(validate_lockfile(source).is_err());
    }
    for field in ["name", "version", "source", "dependencies"] {
        let mut document = fixture();
        package(&mut document, "crypto-bigint")[field] = 42.into();
        rejection(&document, "malformed package record");
    }
    let mut document = fixture();
    package(&mut document, "tokio")["dependencies"] = vec![toml::Value::Integer(42)].into();
    rejection(&document, "malformed package record");
}

#[test]
fn complete_lockfile_passes_both_independent_advisory_guards() {
    let path =
        std::path::Path::new(super::super::super::WORKSPACE_MANIFEST).with_file_name("Cargo.lock");
    super::super::check(&path).expect("complete approved OIDC graph");
}

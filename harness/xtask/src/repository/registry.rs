//! Validates harness metadata against the executable verification catalogue.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use serde_json::Value;

use super::{Repository, Violation};
use crate::verification::catalogue::CHECKS;

const MANIFEST: &str = "harness/manifest.yaml";
const POLICY: &str = "harness/README.md#registry";

pub(super) fn check(repository: &Repository) -> Vec<Violation> {
    let path = repository.root.join(MANIFEST);
    let manifest_error = |message, line| {
        Violation::new(
            MANIFEST,
            line,
            "harness.manifest",
            message,
            "restore the version 1 platform manifest using the documented fields",
            POLICY,
        )
    };
    if !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
        return vec![manifest_error(
            "expected a regular harness manifest file",
            1,
        )];
    }
    let Ok(text) = fs::read_to_string(path) else {
        return vec![manifest_error("could not read the harness manifest", 1)];
    };
    let manifest: Value = match serde_json::from_str(&text) {
        Ok(manifest) => manifest,
        Err(error) => {
            return vec![manifest_error(
                "manifest must use the JSON subset of YAML",
                error.line().max(1),
            )];
        }
    };
    let (Some(guides), Some(sensors)) = (
        manifest["guides"].as_array(),
        manifest["sensors"].as_array(),
    ) else {
        return vec![manifest_error(
            "manifest must contain guide and sensor arrays",
            1,
        )];
    };
    if !has_fields(&manifest, &["schema_version", "owner", "guides", "sensors"])
        || manifest["schema_version"].as_u64() != Some(1)
        || manifest["owner"].as_str() != Some("fleetiq-platform")
    {
        return vec![manifest_error(
            "invalid manifest schema version, owner, or fields",
            1,
        )];
    }

    let mut violations = Vec::new();
    let mut report = |message, fix| {
        violations.push(Violation::new(
            MANIFEST,
            1,
            "harness.registry",
            message,
            fix,
            POLICY,
        ));
    };
    let mut paired = BTreeMap::new();
    for (index, guide) in guides.iter().enumerate() {
        let entry = index + 1;
        let Some(id) = guide["id"].as_str().filter(|id| valid_id(id)) else {
            report(
                format!("guide {entry} has an invalid ID"),
                "use a kebab-case guide ID",
            );
            continue;
        };
        if paired.insert(id, false).is_some() {
            report(
                format!("duplicate guide ID {id}"),
                "register each guide once",
            );
        }
        if !has_fields(guide, &["id", "documentation"])
            || !known_heading(repository, &guide["documentation"])
        {
            report(
                format!("guide {id} must name an existing Markdown heading"),
                "set documentation to the repository-relative path and heading anchor",
            );
        }
    }
    check_sensors(repository, sensors, &mut paired, &mut report);
    for (id, used) in paired {
        if !used {
            report(
                format!("guide {id} has no sensor"),
                "pair a sensor with this guide or remove the registry entry",
            );
        }
    }
    violations
}

fn check_sensors(
    repository: &Repository,
    sensors: &[Value],
    paired: &mut BTreeMap<&str, bool>,
    report: &mut impl FnMut(String, &'static str),
) {
    let expected: BTreeSet<_> = CHECKS.iter().map(|check| check.id).collect();
    let mut registered = BTreeSet::new();
    for (index, sensor) in sensors.iter().enumerate() {
        let entry = index + 1;
        let Some(id) = sensor["id"].as_str().filter(|id| valid_id(id)) else {
            report(
                format!("sensor {entry} has an invalid ID"),
                "use the exact catalogue ID",
            );
            continue;
        };
        if !registered.insert(id) {
            report(
                format!("duplicate sensor ID {id}"),
                "register each catalogue ID once",
            );
        }
        if !expected.contains(id) {
            report(
                format!("unknown sensor ID {id}"),
                "use an ID from cargo xtask verify --list",
            );
        }
        if let Some(used) = sensor["paired_guide"]
            .as_str()
            .and_then(|id| paired.get_mut(id))
        {
            *used = true;
        } else {
            report(
                format!("sensor {id} has an unknown paired guide"),
                "use a registered guide ID",
            );
        }
        if !known_heading(repository, &sensor["remediation"]) {
            report(
                format!("sensor {id} remediation must name an existing Markdown heading"),
                "set remediation to the repository-relative path and heading anchor",
            );
        }
        if sensor["blocking"].as_bool() != Some(true) {
            report(
                format!("sensor {id} must be marked blocking"),
                "set blocking to true because a selected verification check must pass",
            );
        }
        if sensor.as_object().is_some_and(|fields| {
            fields.keys().any(|key| {
                !["id", "paired_guide", "remediation", "blocking"].contains(&key.as_str())
            })
        }) {
            report(
                format!("sensor {id} has an unknown field"),
                "use only the documented metadata fields",
            );
        }
    }
    for id in expected.difference(&registered) {
        report(
            format!("missing sensor {id}"),
            "register the existing catalogue ID",
        );
    }
}

fn has_fields(value: &Value, names: &[&str]) -> bool {
    value.as_object().is_some_and(|fields| {
        fields.len() == names.len() && names.iter().all(|name| fields.contains_key(*name))
    })
}

fn valid_id(id: &str) -> bool {
    id.split('-').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    })
}

fn known_heading(repository: &Repository, value: &Value) -> bool {
    value
        .as_str()
        .and_then(|reference| reference.split_once('#'))
        .is_some_and(|(path, anchor)| {
            repository.document(path).is_some_and(|document| {
                document
                    .headings()
                    .iter()
                    .any(|heading| heading.anchor == anchor)
            })
        })
}

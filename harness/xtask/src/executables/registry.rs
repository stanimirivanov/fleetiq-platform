use std::collections::BTreeSet;

use serde_json::Value;

use super::{
    GUIDE, PATH,
    surfaces::{Surface, Target},
};

pub(super) struct Inventory {
    pub(super) baseline: BTreeSet<Surface>,
    pub(super) registrations: BTreeSet<Surface>,
    pub(super) sensors: BTreeSet<String>,
}

impl Inventory {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self, String> {
        let document: Value =
            serde_json::from_slice(bytes).map_err(|_| invalid("expected JSON"))?;
        if document["schema_version"].as_u64() != Some(1)
            || document.as_object().map(serde_json::Map::len) != Some(3)
        {
            return Err(invalid("expected version 1, baseline, and registrations"));
        }
        let mut sensors = BTreeSet::new();
        let baseline = records(&document["baseline"], true, &mut sensors)?;
        let registrations = records(&document["registrations"], false, &mut sensors)?;
        if !baseline.is_disjoint(&registrations) {
            return Err(invalid("a surface appears in both lists"));
        }
        Ok(Self {
            baseline,
            registrations,
            sensors,
        })
    }
}

fn records(
    value: &Value,
    legacy: bool,
    sensors: &mut BTreeSet<String>,
) -> Result<BTreeSet<Surface>, String> {
    let entries = value
        .as_array()
        .ok_or_else(|| invalid("expected an entry array"))?;
    let mut surfaces = BTreeSet::new();
    for (index, entry) in entries.iter().enumerate() {
        let field = |name| {
            entry[name]
                .as_str()
                .filter(|value| !value.trim().is_empty() && !value.chars().any(char::is_control))
        };
        let (
            Some(path),
            Some(kind),
            Some(detail),
            Some(owner),
            Some(class),
            Some(_purpose),
            Some(disposition),
        ) = (
            field("path"),
            field("kind"),
            entry["target"].as_str(),
            field("owner"),
            field("class"),
            field("purpose"),
            field("disposition"),
        )
        else {
            return Err(invalid(&format!("incomplete entry {}", index + 1)));
        };
        let harness_registration = !legacy && class == "harness";
        let sensor = field("sensor");
        let linked_sensor = sensor.is_some_and(|id| {
            id.bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        });
        if owner != "fleetiq-platform"
            || (harness_registration && !linked_sensor)
            || (!harness_registration && entry.get("sensor").is_some())
            || !matches!(
                class,
                "application" | "operation" | "harness" | "unclassified"
            )
            || (!legacy && class == "unclassified")
            || !matches!(disposition, "retain" | "consolidate" | "review")
            || (class == "unclassified" && disposition != "review")
            || (kind == "script") != detail.is_empty()
            || detail.chars().any(char::is_control)
            || path
                .split('/')
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
            || path.contains(['*', '?', '[', ']'])
            || entry.as_object().map(serde_json::Map::len)
                != Some(7 + usize::from(harness_registration))
        {
            return Err(invalid(&format!("invalid entry {}", index + 1)));
        }
        if let Some(sensor) = sensor {
            sensors.insert(sensor.to_owned());
        }
        let target = match kind {
            "script" => Target::Script,
            "task" => Target::Task(detail.to_owned()),
            "ci" => Target::Inline(detail.to_owned()),
            _ => return Err(invalid("unknown surface kind")),
        };
        if !surfaces.insert(Surface::new(path, target)) {
            return Err(invalid(&format!(
                "duplicate surface at entry {}",
                index + 1
            )));
        }
    }
    Ok(surfaces)
}

fn invalid(reason: &str) -> String {
    format!(
        "EXEC_REGISTRY FAIL {reason} ({PATH})\nFix: correct the exact registration\nRead: {GUIDE}"
    )
}

pub(super) fn validate_sensors(sensors: &BTreeSet<String>, manifest: &[u8]) -> Result<(), String> {
    let value: Value = serde_json::from_slice(manifest)
        .map_err(|_| invalid("expected a harness manifest for sensor links"))?;
    let entries = value["sensors"]
        .as_array()
        .ok_or_else(|| invalid("expected sensors in harness/manifest.yaml"))?;
    for sensor in sensors {
        if !crate::verification::catalogue::CHECKS
            .iter()
            .any(|check| check.id == sensor)
            || !entries.iter().any(|entry| {
                entry["id"].as_str() == Some(sensor.as_str())
                    && entry["paired_guide"]
                        .as_str()
                        .is_some_and(|guide| !guide.is_empty())
            })
        {
            return Err(invalid(&format!("unregistered harness sensor {sensor}")));
        }
    }
    Ok(())
}

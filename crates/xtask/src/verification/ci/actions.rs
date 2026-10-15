//! Action dependency refs live only in the workflow so Dependabot can update
//! them without a second version edit. The template still fixes each action
//! repository and its exact position. No YAML structure is interpreted here.

use std::collections::BTreeMap;

/// Only these upstream repositories may supply the template's action slots.
const ACTIONS: &[(&str, &str)] = &[
    ("checkout", "actions/checkout"),
    ("setup-node", "actions/setup-node"),
    ("upload-artifact", "actions/upload-artifact"),
    ("pnpm", "pnpm/action-setup"),
];

/// Preserves reviewed workflow pins during both checking and explicit rendering.
///
/// A dependency ref may change independently of the workflow structure, but
/// must remain a full commit SHA with a version comment and agree at every use.
/// This validates shape and ownership, not upstream provenance or compatibility;
/// review and the executing CI jobs supply those checks.
pub(super) fn substitute(template: &mut String, workflow: &str) -> Result<(), String> {
    let mut pins = BTreeMap::new();
    for (index, line) in workflow.lines().enumerate() {
        // Canonical step indentation is template-owned. More deeply indented
        // shell/block-scalar payloads must not be interpreted as action refs.
        let Some(reference) = line.strip_prefix("        uses: ") else {
            continue;
        };
        let (repository, pin) = reference.split_once('@').ok_or_else(|| invalid(index))?;
        if !ACTIONS.iter().any(|(_, allowed)| *allowed == repository) || !valid_pin(pin) {
            return Err(invalid(index));
        }
        if let Some(previous) = pins.insert(repository, pin)
            && previous != pin
        {
            return Err(format!(
                "{}:{} uses inconsistent action pins; update every occurrence together; policy: docs/development/verification.md",
                super::WORKFLOW,
                index + 1
            ));
        }
    }
    for (name, repository) in ACTIONS {
        let token = format!("@action:{name}@");
        if !template.contains(&token) {
            return Err(format!("{} is missing {token}", super::TEMPLATE));
        }
        let pin = pins.get(repository).ok_or_else(|| {
            format!(
                "{} is missing a pinned {repository} action; restore its reviewed SHA and version comment before regeneration",
                super::WORKFLOW
            )
        })?;
        *template = template.replace(&token, pin);
    }
    Ok(())
}

fn valid_pin(pin: &str) -> bool {
    let Some((sha, version)) = pin.split_once(" # v") else {
        return false;
    };
    let fields: Vec<_> = version.split('.').collect();
    sha.len() == 40
        && sha
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        && sha.bytes().any(|byte| byte != b'0')
        && fields.len() == 3
        && fields.iter().all(|field| {
            !field.is_empty()
                && field.bytes().all(|byte| byte.is_ascii_digit())
                && (field.len() == 1 || !field.starts_with('0'))
        })
        && fields[0] != "0"
}

fn invalid(index: usize) -> String {
    // Never repeat a supplied action ref: malformed input could contain secrets.
    format!(
        "{}:{} requires an approved action repository, full lowercase commit SHA, and '# v<major>.<minor>.<patch>' comment; policy: docs/development/verification.md",
        super::WORKFLOW,
        index + 1
    )
}

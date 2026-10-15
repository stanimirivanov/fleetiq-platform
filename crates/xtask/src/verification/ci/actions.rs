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

/// Preserves workflow-owned action references during checking and rendering.
///
/// A dependency ref may change independently of the workflow structure, but
/// must remain a major-version tag and agree at every use.
/// This validates shape and ownership, not upstream provenance or compatibility;
/// review and the executing CI jobs supply those checks.
pub(super) fn substitute(template: &mut String, workflow: &str) -> Result<(), String> {
    let mut references = BTreeMap::new();
    for (index, line) in workflow.lines().enumerate() {
        // Canonical step indentation is template-owned. More deeply indented
        // shell/block-scalar payloads must not be interpreted as action refs.
        let Some(reference) = line.strip_prefix("        uses: ") else {
            continue;
        };
        let (repository, tag) = reference.split_once('@').ok_or_else(|| invalid(index))?;
        if !ACTIONS.iter().any(|(_, allowed)| *allowed == repository) || !valid_major_tag(tag) {
            return Err(invalid(index));
        }
        if let Some(previous) = references.insert(repository, tag)
            && previous != tag
        {
            return Err(format!(
                "{}:{} uses inconsistent action tags; update every occurrence together; policy: docs/development/verification.md",
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
        let tag = references.get(repository).ok_or_else(|| {
            format!(
                "{} is missing the {repository} action; restore its major-version tag before regeneration",
                super::WORKFLOW
            )
        })?;
        *template = template.replace(&token, tag);
    }
    Ok(())
}

fn valid_major_tag(tag: &str) -> bool {
    let Some(major) = tag.strip_prefix('v') else {
        return false;
    };
    !major.is_empty() && !major.starts_with('0') && major.bytes().all(|byte| byte.is_ascii_digit())
}

fn invalid(index: usize) -> String {
    // Never repeat a supplied action ref: malformed input could contain secrets.
    format!(
        "{}:{} requires an approved action repository and a major-version tag such as @v7; policy: docs/development/verification.md",
        super::WORKFLOW,
        index + 1
    )
}

//! Aggregate LLVM's source-line summaries by application workspace member.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use cargo_metadata::MetadataCommand;
use serde_json::{Value, json};

pub(super) fn summarize(root: &Path, export: &[u8]) -> Result<Value, &'static str> {
    let root = root
        .canonicalize()
        .map_err(|_| "cannot resolve application workspace")?;
    let metadata = MetadataCommand::new()
        .current_dir(&root)
        .no_deps()
        .other_options(vec!["--locked".into()])
        .exec()
        .map_err(|_| "cannot inventory the locked application workspace")?;
    let members = metadata
        .workspace_packages()
        .into_iter()
        .filter_map(|package| {
            let parent = package
                .manifest_path
                .as_std_path()
                .parent()?
                .strip_prefix(&root)
                .ok()?;
            (!parent.starts_with("harness")).then(|| (parent.join("src"), package.name.to_string()))
        })
        .collect();
    line_counts(&root, &members, export)
}

fn line_counts(
    root: &Path,
    members: &BTreeMap<PathBuf, String>,
    export: &[u8],
) -> Result<Value, &'static str> {
    let report: Value = serde_json::from_slice(export).map_err(|_| "invalid LLVM coverage JSON")?;
    let Some([data]) = report["data"].as_array().map(Vec::as_slice) else {
        return Err("expected one LLVM coverage summary");
    };
    let files = data["files"]
        .as_array()
        .ok_or("missing LLVM file summaries")?;
    let mut counts: BTreeMap<_, _> = members
        .values()
        .map(|name| (name, (0_u64, 0_u64)))
        .collect();
    for file in files {
        let filename = Path::new(file["filename"].as_str().ok_or("missing LLVM filename")?);
        let relative = filename.strip_prefix(root).unwrap_or(filename);
        if relative.starts_with("harness") {
            continue;
        }
        if let Some((_, name)) = members
            .iter()
            .find(|(sources, _)| relative.starts_with(sources))
        {
            let lines = &file["summary"]["lines"];
            let total = lines["count"].as_u64().ok_or("invalid LLVM line count")?;
            let covered = lines["covered"]
                .as_u64()
                .filter(|covered| *covered <= total)
                .ok_or("invalid LLVM covered line count")?;
            let count = counts
                .get_mut(name)
                .expect("workspace member has a coverage row");
            count.0 += total;
            count.1 += covered;
        }
    }
    if counts.values().all(|(total, _)| *total == 0) {
        return Err("LLVM export contains no instrumented application lines");
    }
    Ok(json!(
        counts
            .into_iter()
            .map(|(name, (total, covered))| {
                json!({ "crate": name, "instrumented_lines": total, "covered_lines": covered })
            })
            .collect::<Vec<_>>()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_workspace_source_lines_and_omits_harness_and_tests() {
        let root = Path::new("/workspace");
        let members = BTreeMap::from([
            ("crates/app/src".into(), "app".into()),
            ("packages/guest/src".into(), "guest".into()),
        ]);
        let report = json!({"data": [{"files": [
            {"filename": "/workspace/crates/app/src/lib.rs", "summary": {"lines": {"count": 8, "covered": 6}}},
            {"filename": "crates/app/src/module.rs", "summary": {"lines": {"count": 2, "covered": 1}}},
            {"filename": "crates/app/tests/integration.rs", "summary": {"lines": {"count": 5, "covered": 5}}},
            {"filename": "harness/xtask/src/main.rs", "summary": {"lines": {"count": 10, "covered": 10}}}
        ]}]});
        assert_eq!(
            line_counts(root, &members, &serde_json::to_vec(&report).unwrap()).unwrap(),
            json!([
                {"crate": "app", "instrumented_lines": 10, "covered_lines": 7},
                {"crate": "guest", "instrumented_lines": 0, "covered_lines": 0}
            ])
        );
        assert!(line_counts(root, &members, br#"{"data":[{"files":[]}]}"#).is_err());
    }
}

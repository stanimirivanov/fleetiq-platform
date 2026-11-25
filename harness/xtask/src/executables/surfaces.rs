use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::Path,
};

use sha2::{Digest, Sha256};
use yaml_rust2::{Yaml, YamlLoader};

use super::GUIDE;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Surface {
    path: String,
    target: Target,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Target {
    Script,
    Task(String),
    Inline(String),
}

impl Surface {
    pub(super) fn new(path: &str, target: Target) -> Self {
        Self {
            path: path.to_owned(),
            target,
        }
    }
}

impl fmt::Display for Surface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.target {
            Target::Script => write!(f, "{:?}", self.path),
            Target::Task(name) => write!(f, "{:?}, task {name:?}", self.path),
            Target::Inline(identity) => write!(f, "{:?}, ci {identity:?}", self.path),
        }
    }
}

pub(super) fn is_script(path: &str) -> bool {
    let extension = Path::new(path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase);
    matches!(
        extension.as_deref(),
        Some(
            "ash"
                | "awk"
                | "bash"
                | "bat"
                | "cjs"
                | "cts"
                | "cmd"
                | "fish"
                | "js"
                | "jsx"
                | "ksh"
                | "lua"
                | "mjs"
                | "mts"
                | "nu"
                | "php"
                | "pl"
                | "ps1"
                | "py"
                | "rb"
                | "sh"
                | "ts"
                | "zsh"
        )
    )
}

pub(super) fn has_shebang(bytes: &[u8]) -> bool {
    bytes
        .strip_prefix(b"#!")
        .and_then(|line| line.split(|byte| *byte == b'\n').next())
        .and_then(|line| line.iter().find(|byte| !byte.is_ascii_whitespace()))
        .is_some_and(|byte| *byte != b'[') // Rust crate attributes begin with #![.
}

pub(super) fn is_configuration(path: &str) -> bool {
    let leaf = path.rsplit('/').next().unwrap_or(path);
    leaf == "package.json"
        || path
            .strip_suffix("/config.toml")
            .or_else(|| path.strip_suffix("/config"))
            .is_some_and(|parent| parent.rsplit('/').next() == Some(".cargo"))
        || (path.starts_with(".github/workflows/")
            && path.split('/').count() == 3
            && Path::new(leaf).extension().is_some_and(|extension| {
                extension.eq_ignore_ascii_case("yaml") || extension.eq_ignore_ascii_case("yml")
            }))
        || matches!(
            leaf,
            "action.yml"
                | "action.yaml"
                | "Makefile"
                | "makefile"
                | "GNUmakefile"
                | "Justfile"
                | "justfile"
                | "Taskfile.yml"
                | "Taskfile.yaml"
                | "taskfile.yml"
                | "taskfile.yaml"
        )
}

pub(super) fn configuration(
    found: &mut BTreeSet<Surface>,
    path: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let failed = || {
        format!(
            "EXEC_REGISTRY FAIL could not inventory executable configuration ({path:?})\nFix: restore a valid task or workflow config\nRead: {GUIDE}"
        )
    };
    let leaf = path.rsplit('/').next().unwrap_or(path);
    match leaf {
        "package.json" => {
            let document: serde_json::Value =
                serde_json::from_slice(bytes).map_err(|_| failed())?;
            if let Some(scripts) = document.get("scripts") {
                for target in scripts.as_object().ok_or_else(failed)?.keys() {
                    found.insert(Surface::new(path, Target::Task(target.to_owned())));
                }
            }
        }
        "config" | "config.toml" => {
            let source = std::str::from_utf8(bytes).map_err(|_| failed())?;
            let document = source.parse::<toml::Table>().map_err(|_| failed())?;
            if let Some(aliases) = document.get("alias") {
                for target in aliases.as_table().ok_or_else(failed)?.keys() {
                    found.insert(Surface::new(path, Target::Task(target.to_owned())));
                }
            }
        }
        "Makefile" | "makefile" | "GNUmakefile" | "Justfile" | "justfile" | "Taskfile.yml"
        | "Taskfile.yaml" | "taskfile.yml" | "taskfile.yaml" => {
            // Platform has no such runner today. Register the complete file
            // revision until this policy gains a parser for that task format.
            found.insert(Surface::new(
                path,
                Target::Task(format!("definition:{}", digest(bytes))),
            ));
        }
        _ => {
            let text = std::str::from_utf8(bytes).map_err(|_| failed())?;
            let docs = YamlLoader::load_from_str(text).map_err(|_| failed())?;
            let [workflow] = docs.as_slice() else {
                return Err(failed());
            };
            if matches!(leaf, "action.yml" | "action.yaml") {
                workflow_steps(
                    found,
                    path,
                    "composite",
                    &workflow["runs"]["steps"],
                    &failed,
                )?;
            } else {
                let jobs = workflow["jobs"].as_hash().ok_or_else(failed)?;
                for (job, config) in jobs {
                    let job = job.as_str().ok_or_else(failed)?;
                    workflow_steps(found, path, job, &config["steps"], &failed)?;
                }
            }
        }
    }
    Ok(())
}

fn workflow_steps(
    found: &mut BTreeSet<Surface>,
    path: &str,
    job: &str,
    steps: &Yaml,
    failed: &impl Fn() -> String,
) -> Result<(), String> {
    if steps.is_badvalue() {
        return Ok(()); // Reusable-workflow calls and non-composite actions have no steps.
    }
    let mut occurrences = BTreeMap::new();
    for step in steps.as_vec().ok_or_else(failed)? {
        let body = if !step["run"].is_badvalue() {
            Some(("run", &step["run"]))
        } else if step["uses"].as_str().is_some_and(|action| {
            action
                .to_ascii_lowercase()
                .starts_with("actions/github-script@")
        }) {
            Some(("github-script", &step["with"]["script"]))
        } else {
            None
        };
        if let Some((kind, value)) = body {
            let script = value.as_str().ok_or_else(failed)?.trim();
            let identity = format!("{job}:{kind}:{}", digest(script.as_bytes()));
            let count = occurrences.entry(identity.clone()).or_insert(0);
            *count += 1;
            found.insert(Surface::new(
                path,
                Target::Inline(format!("{identity}:{count}")),
            ));
        }
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| {
            [
                char::from(DIGITS[usize::from(byte >> 4)]),
                char::from(DIGITS[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

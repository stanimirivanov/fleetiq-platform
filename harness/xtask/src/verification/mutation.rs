//! Run the bounded domain pilot and publish only validated mutation metadata.

use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

use super::{catalogue::Tool, runner::Outcome};

#[derive(Clone, Copy)]
pub(super) enum Target {
    Relationship,
    Identifier,
}

impl Target {
    const fn name(self) -> &'static str {
        match self {
            Self::Relationship => "relationship",
            Self::Identifier => "identifier",
        }
    }

    pub(super) const fn file(self) -> &'static str {
        match self {
            Self::Relationship => "crates/domain/src/relationship.rs",
            Self::Identifier => "crates/domain/src/identifier.rs",
        }
    }

    const fn control(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Relationship => (
                "interval_control_caught",
                "EffectiveInterval::contains",
                "replace < with <= in EffectiveInterval::contains",
            ),
            Self::Identifier => (
                "identifier_limit_control_caught",
                "validate",
                "replace > with >= in validate",
            ),
        }
    }
}

pub(super) fn run(command: &mut Command, target: Target) -> Result<Value, Outcome> {
    let scratch = Scratch::create().map_err(not_run)?;
    for (name, _) in env::vars_os() {
        if name.to_string_lossy().starts_with("CARGO_MUTANTS_") {
            command.env_remove(name);
        }
    }
    let status = command
        .env("CARGO_MUTANTS_OUTPUT", &scratch.0)
        .env("RUSTFLAGS", "-D warnings")
        .env("RUSTDOCFLAGS", "-D warnings")
        .status()
        .map_err(|_| {
            not_run("could not start the bounded mutation command; install GNU timeout")
        })?;

    let code = status.code();
    let failure = Outcome::Failed { exit_code: code };
    match code {
        Some(0 | 2 | 3) => {}
        Some(4) => {
            eprintln!("mutation pilot incomplete: the unmodified crate baseline failed");
            return Err(failure);
        }
        Some(124 | 137) => {
            eprintln!("mutation pilot incomplete: the whole-run deadline was reached");
            return Err(failure);
        }
        _ => {
            eprintln!(
                "mutation pilot incomplete: the mutation process did not finish successfully"
            );
            return Err(failure);
        }
    }

    let output = scratch.0.join("mutants.out");
    let report = read_json(&output.join("outcomes.json")).and_then(|outcomes| {
        let selected = read_json(&output.join("mutants.json"))?;
        summarize(target, &selected, &outcomes, code)
    });
    report.map_err(|reason| {
        eprintln!("mutation pilot incomplete: {reason}");
        failure
    })
}

fn not_run(reason: &str) -> Outcome {
    Outcome::NotRun {
        reason: reason.into(),
    }
}

fn read_json(path: &Path) -> Result<Value, &'static str> {
    let bytes = fs::read(path).map_err(|_| "missing mutation evidence")?;
    serde_json::from_slice(&bytes).map_err(|_| "invalid mutation evidence")
}

fn name(target: Target, mutant: &Value) -> Option<&str> {
    mutant["name"].as_str().filter(|name| {
        mutant["file"] == target.file()
            && mutant["package"] == "fleetiq-domain"
            && name
                .strip_prefix(target.file())
                .is_some_and(|tail| tail.starts_with(':'))
            && !name.chars().any(char::is_control)
    })
}

fn summarize(
    target: Target,
    selected: &Value,
    report: &Value,
    code: Option<i32>,
) -> Result<Value, &'static str> {
    if report["cargo_mutants_version"] != Tool::Mutants.expected()
        || report["end_time"].as_str().is_none_or(str::is_empty)
    {
        return Err("mutation report lacks the pinned version or a final timestamp");
    }
    let selected = selected.as_array().ok_or("missing selected mutations")?;
    let mut remaining = selected
        .iter()
        .map(|mutant| name(target, mutant))
        .collect::<Option<BTreeSet<_>>>()
        .filter(|names| !names.is_empty() && names.len() == selected.len())
        .ok_or("selected mutations are empty, out of scope or duplicated")?;
    let selected_mutations: Vec<_> = remaining.iter().copied().collect();
    let [baseline, mutants @ ..] = report["outcomes"].as_array().map_or(&[][..], Vec::as_slice)
    else {
        return Err("missing baseline outcome");
    };
    if baseline["scenario"] != "Baseline"
        || baseline["summary"] != "Success"
        || baseline["phase_results"]
            .as_array()
            .and_then(|phases| phases.last())
            .is_none_or(|phase| phase["phase"] != "Test")
    {
        return Err("the unmodified crate test suite did not pass");
    }

    let (mut caught, mut missed, mut unviable, mut timed_out) = (0_u64, Vec::new(), 0_u64, 0_u64);
    let (control_field, control_function, control_suffix) = target.control();
    let mut control_caught = false;
    for outcome in mutants {
        let mutant = &outcome["scenario"]["Mutant"];
        let name = name(target, mutant)
            .filter(|name| remaining.remove(name))
            .ok_or("outcome does not match a unique selected mutation")?;
        match outcome["summary"].as_str() {
            Some("CaughtMutant") => {
                caught += 1;
                control_caught |= mutant["function"]["function_name"] == control_function
                    && name.ends_with(control_suffix);
            }
            Some("MissedMutant") => missed.push(name),
            Some("Unviable") => unviable += 1,
            Some("Timeout") => timed_out += 1,
            _ => return Err("unknown mutation outcome"),
        }
    }
    let expected_code = if timed_out > 0 {
        3
    } else if missed.is_empty() {
        0
    } else {
        2
    };
    if !remaining.is_empty()
        || code != Some(expected_code)
        || report["total_mutants"].as_u64() != u64::try_from(selected.len()).ok()
        || report["caught"] != caught
        || report["missed"] != missed.len()
        || report["unviable"] != unviable
        || report["timeout"] != timed_out
    {
        return Err("mutation report does not account for every selected case");
    }
    missed.sort_unstable();
    Ok(json!({
        "target": target.name(),
        "baseline_passed": true,
        "selected": selected.len(),
        "caught": caught,
        "missed": missed.len(),
        "unviable": unviable,
        "timed_out": timed_out,
        (control_field): control_caught,
        "selected_mutations": selected_mutations,
        "missed_mutations": missed,
    }))
}

struct Scratch(PathBuf);

impl Scratch {
    fn create() -> Result<Self, &'static str> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "could not timestamp the mutation scratch directory")?;
        let directory = env::temp_dir().join(format!(
            "fleetiq-mutation-{}-{}",
            std::process::id(),
            stamp.as_nanos()
        ));
        fs::create_dir(&directory).map_err(|_| "could not create a fresh mutation directory")?;
        Ok(Self(directory))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if fs::remove_dir_all(&self.0).is_err() {
            eprintln!("could not clean up the mutation scratch directory");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_finished_runs_with_a_passing_baseline_account_for_every_mutation() {
        let target = Target::Relationship;
        let file = target.file();
        let control = json!({
            "name": format!("{file}:130:68: replace < with <= in EffectiveInterval::contains"),
            "file": file,
            "package": "fleetiq-domain",
            "function": { "function_name": "EffectiveInterval::contains" },
        });
        let survivor = json!({
            "name": format!("{file}:21:9: replace UnixTimeMillis::get -> i64 with 0"),
            "file": file,
            "package": "fleetiq-domain",
        });
        let selected = json!([control, survivor]);
        let mut report = json!({
            "cargo_mutants_version": Tool::Mutants.expected(),
            "end_time": "2026-11-25T12:00:00Z",
            "total_mutants": 2,
            "caught": 1,
            "missed": 1,
            "timeout": 0,
            "unviable": 0,
            "outcomes": [
                { "scenario": "Baseline", "summary": "Success", "phase_results": [{ "phase": "Test" }] },
                { "scenario": { "Mutant": control }, "summary": "CaughtMutant" },
                { "scenario": { "Mutant": survivor }, "summary": "MissedMutant" },
            ],
        });
        let receipt = summarize(target, &selected, &report, Some(2)).unwrap();
        assert_eq!(receipt["target"], "relationship");
        assert_eq!(receipt["baseline_passed"], true);
        assert_eq!(
            receipt["selected_mutations"],
            json!([control["name"], survivor["name"]])
        );
        assert_eq!(receipt["interval_control_caught"], true);
        assert_eq!(receipt["missed_mutations"][0], survivor["name"]);
        assert!(summarize(target, &selected, &report, Some(0)).is_err());

        report["outcomes"][2]["summary"] = json!("Timeout");
        report["missed"] = json!(0);
        report["timeout"] = json!(1);
        assert_eq!(
            summarize(target, &selected, &report, Some(3)).unwrap()["timed_out"],
            1
        );

        let mut incomplete = report.clone();
        incomplete["outcomes"].as_array_mut().unwrap().pop();
        assert!(summarize(target, &selected, &incomplete, Some(3)).is_err());

        let mut interrupted = report.clone();
        interrupted["end_time"] = Value::Null;
        assert!(summarize(target, &selected, &interrupted, Some(3)).is_err());

        report["outcomes"][0]["summary"] = json!("Failure");
        assert!(summarize(target, &selected, &report, Some(3)).is_err());
    }

    #[test]
    fn identifier_evidence_records_the_exact_limit_control_for_its_own_target() {
        let control = json!({
            "name": "crates/domain/src/identifier.rs:291:15: replace > with >= in validate",
            "file": "crates/domain/src/identifier.rs",
            "package": "fleetiq-domain",
            "function": { "function_name": "validate" },
        });
        let selected = json!([control]);
        let mut report = json!({
            "cargo_mutants_version": Tool::Mutants.expected(),
            "end_time": "2026-11-25T12:00:00Z",
            "total_mutants": 1,
            "caught": 1,
            "missed": 0,
            "timeout": 0,
            "unviable": 0,
            "outcomes": [
                { "scenario": "Baseline", "summary": "Success", "phase_results": [{ "phase": "Test" }] },
                { "scenario": { "Mutant": control }, "summary": "CaughtMutant" },
            ],
        });

        let receipt = summarize(Target::Identifier, &selected, &report, Some(0)).unwrap();
        assert_eq!(receipt["target"], "identifier");
        assert_eq!(receipt["identifier_limit_control_caught"], true);
        assert_eq!(receipt["selected_mutations"], json!([control["name"]]));
        assert!(summarize(Target::Relationship, &selected, &report, Some(0)).is_err());

        report["outcomes"][1]["summary"] = json!("MissedMutant");
        report["caught"] = json!(0);
        report["missed"] = json!(1);
        let receipt = summarize(Target::Identifier, &selected, &report, Some(2)).unwrap();
        assert_eq!(receipt["identifier_limit_control_caught"], false);
        assert_eq!(receipt["missed_mutations"], json!([control["name"]]));
    }
}

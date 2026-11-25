//! Group execution continues independent checks and never promotes a skip.

use std::{path::Path, time::Instant};

use super::{
    catalogue::{Check, Group},
    evidence,
    process::System,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Passed,
    Failed { exit_code: Option<i32> },
    NotRun { reason: String },
}

/// The small process boundary permits deterministic failure fixtures without
/// replacing the actual commands, prerequisites, or inventory in production.
pub(super) trait Executor {
    fn prerequisite(&mut self, check: &Check) -> Result<(), String>;
    fn execute(&mut self, check: &Check) -> Outcome;
}

pub(super) struct ResultEntry {
    pub(super) check: &'static Check,
    pub(super) outcome: Outcome,
    pub(super) elapsed_ms: u128,
}

fn evaluate(checks: &[&'static Check], executor: &mut impl Executor) -> Vec<ResultEntry> {
    checks
        .iter()
        .map(|check| {
            let started = Instant::now();
            let outcome = match executor.prerequisite(check) {
                Ok(()) => executor.execute(check),
                Err(reason) => Outcome::NotRun { reason },
            };
            println!(
                "{}: {}",
                check.id,
                match &outcome {
                    Outcome::Passed => "passed",
                    Outcome::Failed { .. } => "failed",
                    Outcome::NotRun { .. } => "not run",
                }
            );
            if let Outcome::NotRun { reason } = &outcome {
                println!("  {reason}");
            }
            ResultEntry {
                check,
                outcome,
                elapsed_ms: started.elapsed().as_millis(),
            }
        })
        .collect()
}

pub(super) fn run(root: &Path, group: Group, base: Option<&str>) -> Result<bool, String> {
    let checks: Vec<_> = group.checks().collect();
    let mut system = System::new(root, base, &checks);
    let entries = evaluate(&checks, &mut system);
    let consistency = system.finish();
    let passed =
        consistency.is_ok() && entries.iter().all(|entry| entry.outcome == Outcome::Passed);
    let mut report = system.report(group, &entries, passed);
    report["run_blocker"] = serde_json::json!(consistency.err());
    let path = evidence::write(root, group, &report)?;
    println!("verification evidence: {path}");
    if !passed {
        eprintln!("verification incomplete: inspect failed/not-run entries; no acceptance pass");
    }
    Ok(passed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verification::catalogue::CHECKS;

    struct Fixture {
        blocked: Option<&'static str>,
        failures: Vec<&'static str>,
        executed: Vec<&'static str>,
    }

    impl Executor for Fixture {
        fn prerequisite(&mut self, check: &Check) -> Result<(), String> {
            if self.blocked == Some(check.id) {
                Err("required fixture tool is unavailable".into())
            } else {
                Ok(())
            }
        }
        fn execute(&mut self, check: &Check) -> Outcome {
            self.executed.push(check.id);
            if self.failures.contains(&check.id) {
                Outcome::Failed {
                    exit_code: Some(17),
                }
            } else {
                Outcome::Passed
            }
        }
    }

    #[test]
    fn missing_prerequisite_never_executes_or_passes_and_independent_checks_continue() {
        let checks: Vec<_> = CHECKS.iter().take(3).collect();
        let mut fixture = Fixture {
            blocked: Some(checks[1].id),
            failures: vec![],
            executed: vec![],
        };
        let results = evaluate(&checks, &mut fixture);
        assert!(matches!(results[1].outcome, Outcome::NotRun { .. }));
        assert_eq!(fixture.executed, vec![checks[0].id, checks[2].id]);
        assert_eq!(results[2].outcome, Outcome::Passed);
    }

    #[test]
    fn failed_exit_remains_failed_without_retry_and_later_checks_continue() {
        let checks: Vec<_> = CHECKS.iter().take(2).collect();
        let mut fixture = Fixture {
            blocked: None,
            failures: vec![checks[0].id],
            executed: vec![],
        };
        let results = evaluate(&checks, &mut fixture);
        assert_eq!(
            results[0].outcome,
            Outcome::Failed {
                exit_code: Some(17)
            }
        );
        assert_eq!(results[1].outcome, Outcome::Passed);
        assert_eq!(fixture.executed.len(), 2);
    }

    #[test]
    fn signal_termination_has_no_invented_numeric_exit_code() {
        assert_eq!(
            evidence::outcome(&Outcome::Failed { exit_code: None })["exit_code"],
            serde_json::Value::Null
        );
    }
}

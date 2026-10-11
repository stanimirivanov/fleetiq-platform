//! Authoritative commands and prerequisites for reproducible verification.
//!
//! Commands are executable argument vectors. Display text is the exact
//! contributor gate, never shell input; only the trusted base placeholder is
//! substituted by the runner. Focused groups do not replace acceptance.

/// Selects an explicit gate without guessing scope from changed files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Group {
    Architecture,
    SupplyChain,
    Rust,
    Openapi,
    Acceptance,
}

impl Group {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "architecture" => Some(Self::Architecture),
            "supply-chain" => Some(Self::SupplyChain),
            "rust" => Some(Self::Rust),
            "openapi" => Some(Self::Openapi),
            "acceptance" => Some(Self::Acceptance),
            _ => None,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Architecture => "architecture",
            Self::SupplyChain => "supply-chain",
            Self::Rust => "rust",
            Self::Openapi => "openapi",
            Self::Acceptance => "acceptance",
        }
    }

    /// Keeps acceptance in canonical order, including every required check.
    pub(crate) fn checks(self) -> impl Iterator<Item = &'static Check> {
        CHECKS.iter().filter(move |check| {
            if self == Self::Acceptance {
                check.acceptance
            } else {
                check.group == self
            }
        })
    }
}

/// A condition needed to execute a check, distinct from a passing result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Prerequisite {
    TrustedBase,
    Database,
    OpenapiDependencies,
}

impl Prerequisite {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::TrustedBase => "trusted-base",
            Self::Database => "database",
            Self::OpenapiDependencies => "openapi-dependencies",
        }
    }

    pub(crate) const fn description(self) -> &'static str {
        match self {
            Self::TrustedBase => "An explicit trusted base resolving to an ancestor commit",
            Self::Database => {
                "DATABASE_URL for fully migrated PostgreSQL 18.6; the real gates prove schema compatibility"
            }
            Self::OpenapiDependencies => {
                "Locked OpenAPI dependencies installed with pnpm install --frozen-lockfile"
            }
        }
    }
}

/// Tool identity and reviewed version policy, shared by execution and evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool {
    Rust,
    Git,
    Gitleaks,
    Deny,
    Machete,
    Sqlx,
    Node,
    Pnpm,
}

impl Tool {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Git => "git",
            Self::Gitleaks => "gitleaks",
            Self::Deny => "cargo-deny",
            Self::Machete => "cargo-machete",
            Self::Sqlx => "sqlx",
            Self::Node => "node",
            Self::Pnpm => "pnpm",
        }
    }

    /// Exact pins except Node's reviewed major and Git's installed-tool policy.
    pub(crate) const fn expected(self) -> &'static str {
        match self {
            Self::Rust => "1.98.1",
            Self::Git => "installed",
            Self::Gitleaks => crate::secrets::VERSION,
            Self::Deny => "0.20.2",
            Self::Machete => "0.9.2",
            Self::Sqlx => "0.9.0",
            Self::Node => "24",
            Self::Pnpm => "12.5.1",
        }
    }
}

/// Immutable check definition; the runner records results separately.
#[derive(Debug)]
pub(crate) struct Check {
    pub(crate) id: &'static str,
    pub(crate) group: Group,
    pub(crate) program: &'static str,
    pub(crate) args: &'static [&'static str],
    pub(crate) command: &'static str,
    pub(crate) outcome: &'static str,
    pub(crate) prerequisites: &'static [Prerequisite],
    pub(crate) tools: &'static [Tool],
    pub(crate) acceptance: bool,
}

/// The twelve acceptance checks plus focused sensor tests and `OpenAPI` checks.
///
/// `@base` is an argument placeholder, never accepted as a revision itself.
/// Supplemental checks remain explicit so focused success cannot imply that
/// the database-backed acceptance gate ran.
pub(crate) static CHECKS: &[Check] = &[
    Check {
        id: "repository",
        group: Group::Architecture,
        program: "cargo",
        args: &["xtask", "repository"],
        command: "cargo xtask repository",
        outcome: "Repository documentation, review templates, and policy consistency",
        prerequisites: &[],
        tools: &[Tool::Rust],
        acceptance: true,
    },
    Check {
        id: "architecture",
        group: Group::Architecture,
        program: "cargo",
        args: &["xtask", "architecture"],
        command: "cargo xtask architecture",
        outcome: "Exact capability dependencies, external ownership, and inherited workspace lints",
        prerequisites: &[],
        tools: &[Tool::Rust],
        acceptance: true,
    },
    Check {
        id: "migrations",
        group: Group::Architecture,
        program: "cargo",
        args: &["xtask", "migrations", "--base", "@base"],
        command: "cargo xtask migrations --base origin/main",
        outcome: "Published SQL migration bytes and appended version inventory",
        prerequisites: &[Prerequisite::TrustedBase],
        tools: &[Tool::Rust, Tool::Git],
        acceptance: true,
    },
    Check {
        id: "secrets",
        group: Group::Architecture,
        program: "cargo",
        args: &["xtask", "secrets", "--base", "@base"],
        command: "cargo xtask secrets --base origin/main",
        outcome: "Reviewable files, raw index blobs, and trusted-base incoming history secret protection",
        prerequisites: &[Prerequisite::TrustedBase],
        tools: &[Tool::Rust, Tool::Git, Tool::Gitleaks],
        acceptance: true,
    },
    Check {
        id: "secret-fixtures",
        group: Group::Architecture,
        program: "cargo",
        args: &["xtask", "secrets", "--fixtures"],
        command: "cargo xtask secrets --fixtures",
        outcome: "Real scanner suppression, exact exception, index, and history bypass fixtures",
        prerequisites: &[],
        tools: &[Tool::Rust, Tool::Git, Tool::Gitleaks],
        acceptance: true,
    },
    Check {
        id: "deny",
        group: Group::SupplyChain,
        program: "cargo",
        args: &["deny", "check"],
        command: "cargo deny check",
        outcome: "Current advisories, licenses, sources, and dependency policy against the locked graph",
        prerequisites: &[],
        tools: &[Tool::Rust, Tool::Deny],
        acceptance: true,
    },
    Check {
        id: "machete",
        group: Group::SupplyChain,
        program: "cargo",
        args: &["machete", "--with-metadata", "--skip-target-dir"],
        command: "cargo machete --with-metadata --skip-target-dir",
        outcome: "Unused direct dependency findings using Cargo metadata",
        prerequisites: &[],
        tools: &[Tool::Rust, Tool::Machete],
        acceptance: true,
    },
    Check {
        id: "fmt",
        group: Group::Rust,
        program: "cargo",
        args: &["fmt", "--all", "--check"],
        command: "cargo fmt --all --check",
        outcome: "Workspace Rust formatting",
        prerequisites: &[],
        tools: &[Tool::Rust],
        acceptance: true,
    },
    Check {
        id: "sqlx",
        group: Group::Rust,
        program: "cargo",
        args: &[
            "sqlx",
            "prepare",
            "--check",
            "--workspace",
            "--",
            "--all-targets",
            "--all-features",
        ],
        command: "cargo sqlx prepare --check --workspace -- --all-targets --all-features",
        outcome: "Committed offline query metadata against the fully migrated live schema",
        prerequisites: &[Prerequisite::Database],
        tools: &[Tool::Rust, Tool::Sqlx],
        acceptance: true,
    },
    Check {
        id: "clippy",
        group: Group::Rust,
        program: "cargo",
        args: &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
        command: "cargo clippy --workspace --all-targets --all-features -- -D warnings",
        outcome: "All-target, all-feature workspace lint gate with denied warnings",
        prerequisites: &[Prerequisite::Database],
        tools: &[Tool::Rust],
        acceptance: true,
    },
    Check {
        id: "tests",
        group: Group::Rust,
        program: "cargo",
        args: &["test", "--workspace", "--all-features", "--locked"],
        command: "cargo test --workspace --all-features --locked",
        outcome: "Locked workspace behavior including real PostgreSQL integration tests",
        prerequisites: &[Prerequisite::Database],
        tools: &[Tool::Rust],
        acceptance: true,
    },
    Check {
        id: "docs",
        group: Group::Rust,
        program: "cargo",
        args: &["doc", "--workspace", "--no-deps", "--locked"],
        command: "cargo doc --workspace --no-deps --locked",
        outcome: "Workspace rustdoc gate with dependency documentation excluded",
        prerequisites: &[Prerequisite::Database],
        tools: &[Tool::Rust],
        acceptance: true,
    },
    Check {
        id: "sensor-tests",
        group: Group::Architecture,
        program: "cargo",
        args: &["test", "-p", "fleetiq-xtask", "--locked"],
        command: "cargo test -p fleetiq-xtask --locked",
        outcome: "Repository sensor positive, negative, and bypass fixtures",
        prerequisites: &[],
        tools: &[Tool::Rust, Tool::Git],
        acceptance: false,
    },
    Check {
        id: "openapi",
        group: Group::Openapi,
        program: "pnpm",
        args: &["check:openapi"],
        command: "pnpm check:openapi",
        outcome: "Modular contract linting and byte-for-byte committed bundle consistency",
        prerequisites: &[Prerequisite::OpenapiDependencies],
        tools: &[Tool::Node, Tool::Pnpm, Tool::Git],
        acceptance: false,
    },
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{CHECKS, Group, Prerequisite, Tool};

    #[test]
    fn acceptance_preserves_the_complete_canonical_gate_in_order() {
        let commands: Vec<_> = Group::Acceptance
            .checks()
            .map(|check| check.command)
            .collect();
        assert_eq!(
            commands,
            [
                "cargo xtask repository",
                "cargo xtask architecture",
                "cargo xtask migrations --base origin/main",
                "cargo xtask secrets --base origin/main",
                "cargo xtask secrets --fixtures",
                "cargo deny check",
                "cargo machete --with-metadata --skip-target-dir",
                "cargo fmt --all --check",
                "cargo sqlx prepare --check --workspace -- --all-targets --all-features",
                "cargo clippy --workspace --all-targets --all-features -- -D warnings",
                "cargo test --workspace --all-features --locked",
                "cargo doc --workspace --no-deps --locked",
            ]
        );
    }

    #[test]
    fn inventory_ids_are_unique_and_every_explicit_group_has_checks() {
        assert_eq!(CHECKS.len(), 14);
        let ids: BTreeSet<_> = CHECKS.iter().map(|check| check.id).collect();
        assert_eq!(ids.len(), CHECKS.len());
        for group in [
            Group::Architecture,
            Group::SupplyChain,
            Group::Rust,
            Group::Openapi,
        ] {
            assert!(group.checks().next().is_some());
            assert_eq!(Group::parse(group.name()), Some(group));
        }
        assert_eq!(
            Group::parse(Group::Acceptance.name()),
            Some(Group::Acceptance)
        );
        assert_eq!(Group::parse("all"), None);
        assert!(CHECKS.iter().all(|check| check.group != Group::Acceptance));
    }

    #[test]
    fn executable_arguments_match_the_canonical_display_commands() {
        for check in CHECKS {
            let arguments: Vec<_> = check
                .args
                .iter()
                .map(|value| {
                    if *value == "@base" {
                        "origin/main"
                    } else {
                        value
                    }
                })
                .collect();
            assert_eq!(
                format!("{} {}", check.program, arguments.join(" ")),
                check.command
            );
            assert!(!check.outcome.is_empty());
            assert!(!check.tools.is_empty());
            assert_eq!(
                check.args.contains(&"@base"),
                check.prerequisites.contains(&Prerequisite::TrustedBase),
            );
        }
    }

    #[test]
    fn focused_sensor_tests_and_openapi_never_claim_acceptance() {
        assert!(
            Group::Architecture
                .checks()
                .any(|check| check.id == "sensor-tests")
        );
        assert_eq!(
            Group::Openapi
                .checks()
                .map(|check| check.id)
                .collect::<Vec<_>>(),
            ["openapi"]
        );
        assert!(
            Group::Acceptance
                .checks()
                .all(|check| check.id != "sensor-tests" && check.id != "openapi")
        );
    }

    #[test]
    fn tool_policies_keep_exact_pins_and_explicit_exceptions() {
        assert_eq!(Tool::Rust.expected(), "1.98.1");
        assert_eq!(Tool::Gitleaks.expected(), crate::secrets::VERSION);
        assert_eq!(Tool::Deny.expected(), "0.20.2");
        assert_eq!(Tool::Machete.expected(), "0.9.2");
        assert_eq!(Tool::Sqlx.expected(), "0.9.0");
        assert_eq!(Tool::Pnpm.expected(), "12.5.1");
        assert_eq!(Tool::Node.expected(), "24");
        assert_eq!(Tool::Git.expected(), "installed");
    }
}

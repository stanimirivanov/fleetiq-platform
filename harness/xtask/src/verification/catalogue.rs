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
    RustStatic,
    RustTests,
    RustIsolation,
    Openapi,
    Acceptance,
}

impl Group {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "architecture" => Some(Self::Architecture),
            "supply-chain" => Some(Self::SupplyChain),
            "rust" => Some(Self::Rust),
            "rust-static" => Some(Self::RustStatic),
            "rust-tests" => Some(Self::RustTests),
            "rust-isolation" => Some(Self::RustIsolation),
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
            Self::RustStatic => "rust-static",
            Self::RustTests => "rust-tests",
            Self::RustIsolation => "rust-isolation",
            Self::Openapi => "openapi",
            Self::Acceptance => "acceptance",
        }
    }

    /// Keeps acceptance in canonical order, including every required check.
    pub(crate) fn checks(self) -> impl Iterator<Item = &'static Check> {
        CHECKS.iter().filter(move |check| match self {
            Self::Acceptance => check.acceptance,
            Self::Rust => matches!(
                check.group,
                Self::RustStatic | Self::RustTests | Self::RustIsolation
            ),
            _ => check.group == self,
        })
    }
}

/// A condition needed to execute a check, distinct from a passing result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Prerequisite {
    TrustedBase,
    Database,
    OpenapiDependencies,
    NextestConfiguration,
    ObjectStoreFixture,
}

impl Prerequisite {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::ObjectStoreFixture => "object-store-fixture",
            Self::TrustedBase => "trusted-base",
            Self::Database => "database",
            Self::OpenapiDependencies => "openapi-dependencies",
            Self::NextestConfiguration => "nextest-configuration",
        }
    }

    pub(crate) const fn description(self) -> &'static str {
        match self {
            Self::ObjectStoreFixture => {
                "Local Linux Docker, GNU timeout and the pinned RustFS archive fixture; tests enforce ownership and cleanup"
            }
            Self::TrustedBase => "An explicit trusted base resolving to an ancestor commit",
            Self::Database => {
                "DATABASE_URL for fully migrated PostgreSQL 18.6; the real gates prove schema compatibility"
            }
            Self::NextestConfiguration => {
                "Reviewed complete-suite Nextest profile and resource limits"
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
    Nextest,
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
            Self::Nextest => "cargo-nextest",
            Self::Node => "node",
            Self::Pnpm => "pnpm",
        }
    }

    /// Exact pins except Node's reviewed major and Git's installed-tool policy.
    pub(crate) const fn expected(self) -> &'static str {
        match self {
            Self::Rust => "1.100.0",
            Self::Git => "installed",
            Self::Gitleaks => crate::secrets::VERSION,
            Self::Deny => "0.20.2",
            Self::Machete => "0.9.2",
            Self::Sqlx => "0.9.0",
            Self::Nextest => "0.9.145",
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

/// Application and harness acceptance checks, plus focused verification.
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
        id: "executables",
        group: Group::Architecture,
        program: "cargo",
        args: &["xtask", "executables", "--base", "@base"],
        command: "cargo xtask executables --base origin/main",
        outcome: "Exact registration of platform scripts, task targets, and inline CI commands",
        prerequisites: &[Prerequisite::TrustedBase],
        tools: &[Tool::Rust, Tool::Git],
        acceptance: false,
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
        id: "app-isolation",
        group: Group::RustIsolation,
        program: "cargo",
        args: &["xtask", "app-isolation"],
        command: "cargo xtask app-isolation",
        outcome: "Locked application build, complete suite and doctests with harness removed",
        prerequisites: &[
            Prerequisite::Database,
            Prerequisite::NextestConfiguration,
            Prerequisite::ObjectStoreFixture,
        ],
        tools: &[Tool::Rust, Tool::Git, Tool::Nextest],
        acceptance: false,
    },
    Check {
        id: "fmt",
        group: Group::RustStatic,
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
        group: Group::RustStatic,
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
        group: Group::RustStatic,
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
        group: Group::RustTests,
        program: "cargo",
        args: &[
            "nextest",
            "run",
            "--workspace",
            "--all-features",
            "--locked",
            "--profile",
            "ci",
        ],
        command: "cargo nextest run --workspace --all-features --locked --profile ci",
        outcome: "Complete workspace behavior, bounded PostgreSQL concurrency and real immutable S3 qualification",
        prerequisites: &[
            Prerequisite::Database,
            Prerequisite::NextestConfiguration,
            Prerequisite::ObjectStoreFixture,
        ],
        tools: &[Tool::Rust, Tool::Nextest],
        acceptance: true,
    },
    Check {
        id: "doctests",
        group: Group::RustTests,
        program: "cargo",
        args: &["test", "--workspace", "--all-features", "--locked", "--doc"],
        command: "cargo test --workspace --all-features --locked --doc",
        outcome: "Workspace documentation examples, including compile-fail authorization contracts",
        prerequisites: &[Prerequisite::Database],
        tools: &[Tool::Rust],
        acceptance: true,
    },
    Check {
        id: "docs",
        group: Group::RustStatic,
        program: "cargo",
        args: &["doc", "--workspace", "--no-deps", "--locked"],
        command: "cargo doc --workspace --no-deps --locked",
        outcome: "Workspace rustdoc gate with dependency documentation excluded",
        prerequisites: &[Prerequisite::Database],
        tools: &[Tool::Rust],
        acceptance: true,
    },
    Check {
        id: "harness-deny",
        group: Group::SupplyChain,
        program: "cargo",
        args: &[
            "deny",
            "--manifest-path",
            "harness/Cargo.toml",
            "--config",
            "deny.toml",
            "check",
        ],
        command: "cargo deny --manifest-path harness/Cargo.toml --config deny.toml check",
        outcome: "Check the independently locked harness for advisories, sources, and licenses",
        prerequisites: &[],
        tools: &[Tool::Rust, Tool::Deny],
        acceptance: true,
    },
    Check {
        id: "harness-machete",
        group: Group::SupplyChain,
        program: "cargo",
        args: &["machete", "--with-metadata", "--skip-target-dir", "harness"],
        command: "cargo machete --with-metadata --skip-target-dir harness",
        outcome: "Check direct harness dependencies for unused declarations",
        prerequisites: &[],
        tools: &[Tool::Rust, Tool::Machete],
        acceptance: true,
    },
    Check {
        id: "harness-fmt",
        group: Group::RustStatic,
        program: "cargo",
        args: &[
            "fmt",
            "--manifest-path",
            "harness/Cargo.toml",
            "--all",
            "--check",
        ],
        command: "cargo fmt --manifest-path harness/Cargo.toml --all --check",
        outcome: "Check harness Rust formatting",
        prerequisites: &[],
        tools: &[Tool::Rust],
        acceptance: true,
    },
    Check {
        id: "harness-clippy",
        group: Group::RustStatic,
        program: "cargo",
        args: &[
            "clippy",
            "--manifest-path",
            "harness/Cargo.toml",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
        command: "cargo clippy --manifest-path harness/Cargo.toml --workspace --all-targets --all-features --locked -- -D warnings",
        outcome: "Check every harness target with denied warnings",
        prerequisites: &[],
        tools: &[Tool::Rust],
        acceptance: true,
    },
    Check {
        id: "harness-docs",
        group: Group::RustStatic,
        program: "cargo",
        args: &[
            "doc",
            "--manifest-path",
            "harness/Cargo.toml",
            "--workspace",
            "--no-deps",
            "--locked",
        ],
        command: "cargo doc --manifest-path harness/Cargo.toml --workspace --no-deps --locked",
        outcome: "Generate harness rustdoc",
        prerequisites: &[],
        tools: &[Tool::Rust],
        acceptance: true,
    },
    Check {
        id: "sensor-tests",
        group: Group::Architecture,
        program: "cargo",
        args: &["test", "--manifest-path", "harness/Cargo.toml", "--locked"],
        command: "cargo test --manifest-path harness/Cargo.toml --locked",
        outcome: "Repository sensor positive, negative, and bypass fixtures",
        prerequisites: &[],
        tools: &[Tool::Rust, Tool::Git],
        acceptance: true,
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
                "cargo nextest run --workspace --all-features --locked --profile ci",
                "cargo test --workspace --all-features --locked --doc",
                "cargo doc --workspace --no-deps --locked",
                "cargo deny --manifest-path harness/Cargo.toml --config deny.toml check",
                "cargo machete --with-metadata --skip-target-dir harness",
                "cargo fmt --manifest-path harness/Cargo.toml --all --check",
                "cargo clippy --manifest-path harness/Cargo.toml --workspace --all-targets --all-features --locked -- -D warnings",
                "cargo doc --manifest-path harness/Cargo.toml --workspace --no-deps --locked",
                "cargo test --manifest-path harness/Cargo.toml --locked",
            ]
        );
    }

    #[test]
    fn inventory_ids_are_unique_and_every_explicit_group_has_checks() {
        assert_eq!(CHECKS.len(), 22);
        let ids: BTreeSet<_> = CHECKS.iter().map(|check| check.id).collect();
        assert_eq!(ids.len(), CHECKS.len());
        for group in [
            Group::Architecture,
            Group::SupplyChain,
            Group::Rust,
            Group::RustStatic,
            Group::RustTests,
            Group::RustIsolation,
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
            assert_ne!(check.outcome.len(), 0);
            assert_ne!(check.tools.len(), 0);
            assert_eq!(
                check.args.contains(&"@base"),
                check.prerequisites.contains(&Prerequisite::TrustedBase),
            );
        }
    }

    #[test]
    fn sensor_tests_remain_required_and_isolation_and_openapi_have_focused_evidence() {
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
                .all(|check| check.id != "app-isolation" && check.id != "openapi")
        );
    }

    #[test]
    fn rust_inventory_matches_toolchain_and_workspace_minimum() {
        let toolchain: toml::Value =
            toml::from_str(include_str!("../../../../rust-toolchain.toml"))
                .expect("valid toolchain manifest");
        let workspace: toml::Value = toml::from_str(include_str!("../../../../Cargo.toml"))
            .expect("valid workspace manifest");
        assert_eq!(
            toolchain["toolchain"]["channel"].as_str(),
            Some(Tool::Rust.expected()),
            "upgrade the toolchain and verification inventory together"
        );
        assert_eq!(
            workspace["workspace"]["package"]["rust-version"].as_str(),
            Some(Tool::Rust.expected()),
            "the workspace minimum must match the reviewed compiler"
        );
    }

    #[test]
    fn tool_policies_keep_exact_pins_and_explicit_exceptions() {
        assert_eq!(Tool::Rust.expected(), "1.100.0");
        assert_eq!(Tool::Gitleaks.expected(), crate::secrets::VERSION);
        assert_eq!(Tool::Deny.expected(), "0.20.2");
        assert_eq!(Tool::Machete.expected(), "0.9.2");
        assert_eq!(Tool::Sqlx.expected(), "0.9.0");
        assert_eq!(Tool::Nextest.expected(), "0.9.145");
        assert_eq!(Tool::Pnpm.expected(), "12.5.1");
        assert_eq!(Tool::Node.expected(), "24");
        assert_eq!(Tool::Git.expected(), "installed");
    }
}

#[cfg(test)]
#[test]
fn rust_shards_partition_the_local_rust_group() {
    let local: std::collections::BTreeSet<_> = Group::Rust.checks().map(|check| check.id).collect();
    let mut hosted = std::collections::BTreeSet::new();
    for group in [Group::RustStatic, Group::RustTests, Group::RustIsolation] {
        assert_eq!(Group::parse(group.name()), Some(group));
        for check in group.checks() {
            assert!(
                hosted.insert(check.id),
                "{} appears in multiple shards",
                check.id
            );
        }
    }
    assert_eq!(hosted, local);
}

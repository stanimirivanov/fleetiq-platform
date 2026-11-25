//! Exact repository command shapes; rejected arguments are never echoed.

use crate::verification::Group;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Command {
    Architecture,
    Repository,
    Executables { base: String },
    Migrations { base: String },
    Secrets { base: Option<String> },
    SecretFixtures,
    AppIsolation,
    VerifyList,
    VerifyCi { write: bool },
    Verify { group: Group, base: Option<String> },
    Help,
}

pub(super) fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Command, ()> {
    let arguments: Vec<String> = arguments.into_iter().collect();
    let fields: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match fields.as_slice() {
        [] | ["help" | "--help" | "-h"] => Ok(Command::Help),
        ["architecture"] => Ok(Command::Architecture),
        ["repository"] => Ok(Command::Repository),
        ["executables", "--base", base] if valid_base(base) => Ok(Command::Executables {
            base: (*base).to_owned(),
        }),
        ["migrations", "--base", base] if valid_base(base) => Ok(Command::Migrations {
            base: (*base).to_owned(),
        }),
        ["secrets"] => Ok(Command::Secrets { base: None }),
        ["secrets", "--base", base] if valid_base(base) => Ok(Command::Secrets {
            base: Some((*base).to_owned()),
        }),
        ["secrets", "--fixtures"] => Ok(Command::SecretFixtures),
        ["app-isolation"] => Ok(Command::AppIsolation),
        ["verify", "--list"] => Ok(Command::VerifyList),
        ["verify", "--check-ci"] => Ok(Command::VerifyCi { write: false }),
        ["verify", "--write-ci"] => Ok(Command::VerifyCi { write: true }),
        ["verify", "--stage", stage, "--base", base] if valid_base(base) => {
            let group = Group::parse_stage(stage).ok_or(())?;
            Ok(Command::Verify {
                group,
                base: Some((*base).to_owned()),
            })
        }
        ["verify", "--group", group] => Group::parse(group)
            .map(|group| Command::Verify { group, base: None })
            .ok_or(()),
        ["verify", "--group", group, "--base", base] if valid_base(base) => Group::parse(group)
            .filter(|group| matches!(group, Group::Architecture | Group::Acceptance))
            .map(|group| Command::Verify {
                group,
                base: Some((*base).to_owned()),
            })
            .ok_or(()),
        _ => Err(()),
    }
}

fn valid_base(base: &str) -> bool {
    !base.is_empty() && !base.starts_with('-') && !base.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::{Command, Group, parse};

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn accepts_only_documented_command_shapes() {
        for (input, expected) in [
            (vec![], Command::Help),
            (vec!["--help"], Command::Help),
            (vec!["architecture"], Command::Architecture),
            (vec!["repository"], Command::Repository),
            (
                vec!["migrations", "--base", "origin/main"],
                Command::Migrations {
                    base: "origin/main".to_owned(),
                },
            ),
            (vec!["secrets"], Command::Secrets { base: None }),
            (
                vec!["secrets", "--base", "base-sha"],
                Command::Secrets {
                    base: Some("base-sha".to_owned()),
                },
            ),
            (vec!["secrets", "--fixtures"], Command::SecretFixtures),
            (vec!["verify", "--list"], Command::VerifyList),
            (
                vec!["verify", "--check-ci"],
                Command::VerifyCi { write: false },
            ),
            (
                vec!["verify", "--write-ci"],
                Command::VerifyCi { write: true },
            ),
            (
                vec!["verify", "--group", "rust"],
                Command::Verify {
                    group: Group::Rust,
                    base: None,
                },
            ),
            (
                vec!["verify", "--group", "architecture", "--base", "origin/main"],
                Command::Verify {
                    group: Group::Architecture,
                    base: Some("origin/main".to_owned()),
                },
            ),
        ] {
            assert_eq!(parse(arguments(&input)), Ok(expected));
        }
    }

    #[test]
    fn edit_stage_requires_an_explicit_trusted_base() {
        assert_eq!(
            parse(arguments(&[
                "verify",
                "--stage",
                "edit",
                "--base",
                "origin/main"
            ])),
            Ok(Command::Verify {
                group: Group::Edit,
                base: Some("origin/main".to_owned())
            })
        );
        for input in [
            vec!["verify", "--stage", "edit"],
            vec!["verify", "--stage", "edit", "--base", "--all"],
            vec!["verify", "--stage", "edit", "--base", ""],
            vec!["verify", "--stage", "pr", "--base", "origin/main"],
            vec!["verify", "--group", "edit"],
        ] {
            assert_eq!(parse(arguments(&input)), Err(()));
        }
    }

    #[test]
    fn commit_stage_requires_an_explicit_trusted_base() {
        assert_eq!(
            parse(arguments(&[
                "verify",
                "--stage",
                "commit",
                "--base",
                "origin/main"
            ])),
            Ok(Command::Verify {
                group: Group::Commit,
                base: Some("origin/main".to_owned())
            })
        );
        for input in [
            vec!["verify", "--stage", "commit"],
            vec!["verify", "--stage", "commit", "--base", "--all"],
            vec!["verify", "--group", "commit"],
        ] {
            assert_eq!(parse(arguments(&input)), Err(()));
        }
    }

    #[test]
    fn executable_guard_requires_an_explicit_base() {
        assert!(parse(arguments(&["executables", "--base", "origin/main"])).is_ok());
        assert!(parse(arguments(&["executables"])).is_err());
    }

    #[test]
    fn rejects_missing_bases_extra_arguments_and_option_injection() {
        for input in [
            vec!["migrations"],
            vec!["migrations", "--base"],
            vec!["migrations", "--base", ""],
            vec!["migrations", "--base", "--all"],
            vec!["secrets", "--base", ""],
            vec!["secrets", "--base", "HEAD\n"],
            vec!["secrets", "--fixtures", "--base", "HEAD"],
            vec!["secrets", "--baseline", "report.json"],
            vec!["help", "extra"],
            vec!["repository", "extra"],
            vec!["architecture", "extra"],
            vec!["verify"],
            vec!["verify", "--group", "anything"],
            vec!["verify", "--group", "rust", "--base", "--all"],
            vec!["verify", "--list", "extra"],
            vec!["verify", "--group", "rust", "--base", "origin/main"],
            vec!["verify", "--group", "rust", "--command", "echo"],
            vec!["verify", "--group", "rust", "--output", "anywhere"],
        ] {
            assert_eq!(parse(arguments(&input)), Err(()));
        }
    }
}

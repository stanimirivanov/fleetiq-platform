//! Exact repository command shapes; rejected arguments are never echoed.

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Command {
    Architecture,
    Repository,
    Migrations { base: String },
    Secrets { base: Option<String> },
    SecretFixtures,
    Help,
}

pub(super) fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Command, ()> {
    let arguments: Vec<String> = arguments.into_iter().collect();
    let fields: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match fields.as_slice() {
        [] | ["help" | "--help" | "-h"] => Ok(Command::Help),
        ["architecture"] => Ok(Command::Architecture),
        ["repository"] => Ok(Command::Repository),
        ["migrations", "--base", base] if valid_base(base) => Ok(Command::Migrations {
            base: (*base).to_owned(),
        }),
        ["secrets"] => Ok(Command::Secrets { base: None }),
        ["secrets", "--base", base] if valid_base(base) => Ok(Command::Secrets {
            base: Some((*base).to_owned()),
        }),
        ["secrets", "--fixtures"] => Ok(Command::SecretFixtures),
        _ => Err(()),
    }
}

fn valid_base(base: &str) -> bool {
    !base.is_empty() && !base.starts_with('-') && !base.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::{Command, parse};

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
        ] {
            assert_eq!(parse(arguments(&input)), Ok(expected));
        }
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
        ] {
            assert_eq!(parse(arguments(&input)), Err(()));
        }
    }
}

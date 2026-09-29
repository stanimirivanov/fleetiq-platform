mod adr;
mod markdown;
mod milestones;
mod templates;

#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

use markdown::Document;

pub(super) const DOCUMENTATION_POLICY: &str = "CONTRIBUTING.md#documentation";
pub(super) const ADR_POLICY: &str = "docs/adr/README.md#lifecycle";
pub(super) const ISSUE_POLICY: &str = "CONTRIBUTING.md#issues-and-milestones";
pub(super) const PULL_REQUEST_POLICY: &str = "CONTRIBUTING.md#pull-request-description";
pub(super) const MILESTONE_POLICY: &str = "docs/roadmap/milestones.md#planning-rules";

const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

#[derive(Debug)]
pub(crate) struct RepositoryReport {
    document_count: usize,
}

impl RepositoryReport {
    pub(crate) const fn document_count(&self) -> usize {
        self.document_count
    }
}

#[derive(Debug)]
pub(crate) enum RepositoryCheckError {
    Operational(String),
    Policy(Vec<Violation>),
}

impl fmt::Display for RepositoryCheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Operational(message) => formatter.write_str(message),
            Self::Policy(violations) => {
                for (index, violation) in violations.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str("\n")?;
                    }
                    write!(formatter, "- {violation}")?;
                }
                Ok(())
            }
        }
    }
}

impl Error for RepositoryCheckError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Violation {
    path: String,
    line: usize,
    rule: &'static str,
    message: String,
    fix: String,
    policy: &'static str,
}

impl Violation {
    pub(super) fn new(
        path: impl Into<String>,
        line: usize,
        rule: &'static str,
        message: impl Into<String>,
        fix: impl Into<String>,
        policy: &'static str,
    ) -> Self {
        Self {
            path: path.into(),
            line,
            rule,
            message: message.into(),
            fix: fix.into(),
            policy,
        }
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}: [{}] {}; fix: {}; policy: {}",
            self.path, self.line, self.rule, self.message, self.fix, self.policy
        )
    }
}

pub(super) struct Repository {
    root: PathBuf,
    documents: BTreeMap<String, Document>,
}

impl Repository {
    pub(super) fn document(&self, path: &str) -> Option<&Document> {
        self.documents.get(path)
    }
}

/// Checks deterministic repository and documentation policy.
pub(crate) fn check() -> Result<RepositoryReport, RepositoryCheckError> {
    check_at(Path::new(WORKSPACE_ROOT))
}

fn check_at(root: &Path) -> Result<RepositoryReport, RepositoryCheckError> {
    let repository = markdown::load(root).map_err(RepositoryCheckError::Operational)?;
    let document_count = repository.documents.len();
    let mut violations = markdown::check_links(&repository);
    violations.extend(markdown::check_tldr(&repository));
    violations.extend(adr::check(&repository));
    violations.extend(milestones::check(&repository));
    violations.extend(templates::check(&repository));
    violations.sort_by(|left, right| {
        (&left.path, left.line, left.rule, &left.message).cmp(&(
            &right.path,
            right.line,
            right.rule,
            &right.message,
        ))
    });

    if violations.is_empty() {
        Ok(RepositoryReport { document_count })
    } else {
        Err(RepositoryCheckError::Policy(violations))
    }
}

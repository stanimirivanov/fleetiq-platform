//! Restricts exceptions to reviewed, exact nonproduction values in exact files.

use toml::Value;

use super::{SecretsError, failure};

pub(super) fn validate(source: &str) -> Result<(), SecretsError> {
    let table: toml::Table = source
        .parse()
        .map_err(|_| failure("secret-scanning configuration is not valid TOML"))?;
    if table
        .keys()
        .any(|key| !matches!(key.as_str(), "title" | "extend" | "allowlists"))
    {
        return Err(failure(
            "secret-scanning policy permits only default rules and exact owned exceptions",
        ));
    }
    let extension = table
        .get("extend")
        .and_then(Value::as_table)
        .ok_or_else(|| failure("secret-scanning policy must extend built-in default rules"))?;
    if extension.len() != 1 || extension.get("useDefault").and_then(Value::as_bool) != Some(true) {
        return Err(failure(
            "secret-scanning policy cannot disable or replace default detector rules",
        ));
    }
    let Some(value) = table.get("allowlists") else {
        return Ok(());
    };
    let exceptions = value
        .as_array()
        .ok_or_else(|| failure("secret-scanning exceptions must be an array of tables"))?;
    for exception in exceptions {
        validate_exception(exception)?;
    }
    Ok(())
}

fn validate_exception(value: &Value) -> Result<(), SecretsError> {
    let table = value
        .as_table()
        .ok_or_else(|| failure("secret-scanning exception must be a table"))?;
    let keys = [
        "description",
        "condition",
        "regexTarget",
        "paths",
        "regexes",
    ];
    if table.len() != keys.len() || table.keys().any(|key| !keys.contains(&key.as_str())) {
        return Err(failure(
            "secret-scanning exceptions require ownership, AND matching, an exact path, and an exact secret",
        ));
    }
    let description = table
        .get("description")
        .and_then(Value::as_str)
        .ok_or_else(|| failure("secret-scanning exception requires a description"))?;
    for label in ["Owner:", "Reason:", "Review trigger:"] {
        let section = description
            .split(label)
            .nth(1)
            .and_then(|text| text.split(';').next())
            .unwrap_or_default()
            .trim();
        if section.is_empty() {
            return Err(failure(
                "secret-scanning exception requires an owner, reason, and review trigger",
            ));
        }
    }
    if table.get("condition").and_then(Value::as_str) != Some("AND")
        || table.get("regexTarget").and_then(Value::as_str) != Some("secret")
    {
        return Err(failure(
            "secret-scanning exceptions must match the exact secret AND exact file",
        ));
    }
    for key in ["paths", "regexes"] {
        let values = table
            .get(key)
            .and_then(Value::as_array)
            .filter(|values| values.len() == 1)
            .ok_or_else(|| {
                failure(
                    "each secret-scanning exception requires one exact path and one exact secret",
                )
            })?;
        let pattern = values[0]
            .as_str()
            .ok_or_else(|| failure("secret-scanning exception patterns must be strings"))?;
        let portable;
        let pattern = if key == "paths" {
            portable = pattern.replace(r"[/\\]", "/");
            portable.as_str()
        } else {
            pattern
        };
        if !is_exact_literal(pattern) {
            return Err(failure(
                "secret-scanning exception patterns must be anchored exact literals, never broad regular expressions",
            ));
        }
    }
    Ok(())
}

/// Accepts an anchored literal with escaped punctuation, not a regular expression
/// language. This prevents a path-only or wildcard exception from widening scope.
pub(super) fn is_exact_literal(pattern: &str) -> bool {
    let Some(body) = pattern
        .strip_prefix('^')
        .and_then(|value| value.strip_suffix('$'))
    else {
        return false;
    };
    if body.is_empty() {
        return false;
    }
    let mut characters = body.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            match characters.next() {
                Some(escaped) if ".+*?()|[]{}^$\\-_".contains(escaped) => {}
                _ => return false,
            }
        } else if ".+*?()|[]{}^$".contains(character) || character.is_control() {
            return false;
        }
    }
    true
}

/// Exact exceptions are interpreted by the wrapper, never passed to Gitleaks.
/// The pinned binary's global allowlist path handling is wider than its declared
/// AND intent; real detector fixtures prevent us from trusting that behavior.
pub(super) struct ExactException {
    path: String,
    secret: String,
}

impl ExactException {
    pub(super) fn matches(&self, path: &str, secret: &str) -> bool {
        self.path == path && self.secret == secret
    }

    pub(super) fn matches_path(&self, path: &str) -> bool {
        self.path == path
    }
}

pub(super) fn exceptions(source: &str) -> Result<Vec<ExactException>, SecretsError> {
    validate(source)?;
    let table: toml::Table = source
        .parse()
        .map_err(|_| failure("secret-scanning configuration is not valid TOML"))?;
    let mut result = Vec::new();
    if let Some(values) = table.get("allowlists").and_then(Value::as_array) {
        for value in values {
            // Shape is already validated; keep errors explicit rather than
            // assuming a malformed value can provide passing evidence.
            let path = value
                .get("paths")
                .and_then(Value::as_array)
                .and_then(|values| values.first())
                .and_then(Value::as_str)
                .ok_or_else(|| failure("exact secret exception lacked its path"))?;
            let secret = value
                .get("regexes")
                .and_then(Value::as_array)
                .and_then(|values| values.first())
                .and_then(Value::as_str)
                .ok_or_else(|| failure("exact secret exception lacked its value"))?;
            result.push(ExactException {
                path: literal_value(&path.replace(r"[/\\]", "/"))?,
                secret: literal_value(secret)?,
            });
        }
    }
    Ok(result)
}

fn literal_value(pattern: &str) -> Result<String, SecretsError> {
    if !is_exact_literal(pattern) {
        return Err(failure("exception value must be an exact anchored literal"));
    }
    let body = &pattern[1..pattern.len() - 1];
    let mut value = String::new();
    let mut characters = body.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            value.push(
                characters
                    .next()
                    .ok_or_else(|| failure("exception had an incomplete literal escape"))?,
            );
        } else {
            value.push(character);
        }
    }
    Ok(value)
}

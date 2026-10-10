//! Checks the two task-justified guidance routes without policing their prose.
//!
//! These guides and the documentation map use plain Markdown for discoverable
//! navigation. Non-comment HTML makes visibility ambiguous, so these three route
//! sources reject it rather than attempt a partial HTML/CSS visibility parser.

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use percent_encoding::percent_decode_str;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use super::{Repository, Violation, markdown::Document};

const POLICY: &str = "docs/development/harness.md#scoped-guidance";
const MAP: &str = "docs/README.md";
const GUIDES: &[Scope] = &[
    Scope {
        path: "crates/xtask/AGENTS.md",
        routes: &[
            "CONTRIBUTING.md",
            "docs/development/engineering-standards.md",
            "docs/development/harness.md",
            "docs/development/harness-protection.md",
            "docs/development/verification.md",
            "docs/architecture/dependency-rules.md",
        ],
    },
    Scope {
        path: "contracts/http/AGENTS.md",
        routes: &[
            "CONTRIBUTING.md",
            "contracts/http/README.md",
            ".agents/skills/openapi/SKILL.md",
            "docs/adr/0048-establish-versioned-http-api-boundary.md",
            "docs/development/verification.md",
        ],
    },
];

struct Scope {
    path: &'static str,
    routes: &'static [&'static str],
}

/// Protects the exact current guidance routes; meaning and task suitability
/// remain review concerns rather than claims made by this mechanical sensor.
pub(super) fn check(repository: &Repository) -> Vec<Violation> {
    let mut violations = Vec::new();
    for scope in GUIDES {
        violations.extend(check_source(repository, scope.path, scope.routes));
    }
    let guides: Vec<_> = GUIDES.iter().map(|scope| scope.path).collect();
    violations.extend(check_source(repository, MAP, &guides));
    violations
}

fn check_source(repository: &Repository, path: &str, required: &[&str]) -> Vec<Violation> {
    let Some(document) = repository.document(path) else {
        return vec![violation(
            path,
            1,
            "required scoped guidance or routing source is missing",
            "restore the exact registered path and its canonical Markdown routes",
        )];
    };
    let routes = rendered_routes(path, document);
    if let Some(line) = routes.raw_html_line {
        return vec![violation(
            path,
            line,
            "scoped navigation sources must use plain Markdown; non-comment raw HTML prevents reliable route visibility checks",
            "replace raw HTML with plain Markdown in this route source; ordinary author comments remain allowed",
        )];
    }
    required
        .iter()
        .filter(|target| !routes.targets.contains(**target))
        .map(|target| {
            violation(
                path,
                1,
                format!("required canonical route to `{target}` is not rendered"),
                "add a visible named Markdown link relative to this file; comments, code, images, and unused reference definitions do not provide navigation",
            )
        })
        .collect()
}

fn violation(
    path: &str,
    line: usize,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Violation {
    Violation::new(path, line, "guidance.routes", message, fix, POLICY)
}

#[derive(Default)]
struct RenderedRoutes {
    targets: BTreeSet<String>,
    raw_html_line: Option<usize>,
}

struct PendingLink {
    destination: String,
    named: bool,
}

/// Inspect actual rendered link events. Reference definitions and image targets
/// are not routes, even though the general broken-link check validates them.
fn rendered_routes(source: &str, document: &Document) -> RenderedRoutes {
    let mut routes = RenderedRoutes::default();
    let mut pending: Option<PendingLink> = None;
    let mut image_depth = 0;
    let mut code_depth = 0;
    let mut comment_open = false;
    let options = Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES;
    for (event, range) in Parser::new_ext(document.body(), options).into_offset_iter() {
        match event {
            Event::Start(Tag::Image { .. }) => image_depth += 1,
            Event::End(TagEnd::Image) => image_depth -= 1,
            Event::Start(Tag::CodeBlock(_)) => code_depth += 1,
            Event::End(TagEnd::CodeBlock) => code_depth -= 1,
            Event::Start(Tag::Link { dest_url, .. }) if image_depth == 0 && code_depth == 0 => {
                pending = Some(PendingLink {
                    destination: dest_url.into_string(),
                    named: false,
                });
            }
            Event::Text(text) | Event::Code(text) if image_depth == 0 && code_depth == 0 => {
                if let Some(link) = &mut pending {
                    link.named |= text.chars().any(char::is_alphanumeric);
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some(link) = pending.take().filter(|link| link.named)
                    && let Some(target) = canonical_destination(source, &link.destination)
                {
                    routes.targets.insert(target);
                }
            }
            Event::Html(html) | Event::InlineHtml(html)
                if !only_comments(&html, &mut comment_open) && routes.raw_html_line.is_none() =>
            {
                routes.raw_html_line = Some(
                    document.body()[..range.start]
                        .bytes()
                        .filter(|byte| *byte == b'\n')
                        .count()
                        + 1,
                );
            }
            _ => {}
        }
    }
    routes
}

/// Comment bodies can arrive as separate HTML events, one per source line.
/// Carry only comment state; any material outside comments is still rejected.
fn only_comments(mut html: &str, comment_open: &mut bool) -> bool {
    loop {
        if *comment_open {
            let Some((_, remainder)) = html.split_once("-->") else {
                return true;
            };
            *comment_open = false;
            html = remainder;
            continue;
        }
        html = html.trim_start();
        if html.is_empty() {
            return true;
        }
        let Some(comment) = html.strip_prefix("<!--") else {
            return false;
        };
        *comment_open = true;
        html = comment;
    }
}

/// Resolve a local forward-slash link lexically. The existing local-link sensor
/// separately verifies exact filesystem casing, existence, and heading anchors.
fn canonical_destination(source: &str, destination: &str) -> Option<String> {
    let path = destination.split(['#', '?']).next()?;
    let path = percent_decode_str(path).decode_utf8().ok()?;
    if path.starts_with('/') || path.contains(['\\', ':']) {
        return None;
    }
    let mut components: Vec<_> = source.rsplit_once('/')?.0.split('/').collect();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop()?;
            }
            component => components.push(component),
        }
    }
    Some(components.join("/"))
}

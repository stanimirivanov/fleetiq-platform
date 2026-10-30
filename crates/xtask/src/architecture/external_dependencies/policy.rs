use super::{DependencyClass, DependencyEdge};

/// Ownership is explicit even when a package has no external dependencies.
/// Keeping kinds separate prevents a test utility becoming a production adapter
/// through a manifest edit. Versions and features belong to dependency review.
#[derive(Clone, Copy)]
pub(super) struct ExternalPackagePolicy {
    pub(super) name: &'static str,
    pub(super) normal: &'static [&'static str],
    pub(super) development: &'static [&'static str],
    pub(super) build: &'static [&'static str],
}

impl ExternalPackagePolicy {
    pub(super) fn dependencies(&self) -> impl Iterator<Item = DependencyEdge> + '_ {
        [
            (DependencyClass::Normal, self.normal),
            (DependencyClass::Development, self.development),
            (DependencyClass::Build, self.build),
        ]
        .into_iter()
        .flat_map(move |(class, dependencies)| {
            dependencies
                .iter()
                .map(move |dependency| DependencyEdge::new(self.name, *dependency, class))
        })
    }
}

const NONE: &[&str] = &[];

// Domain bigdecimal supplies exact value arithmetic without database access.
// Inner application/policy Tokio approvals support async tests only; SQLx and
// Wasmtime stay with their adapters and concrete server composition. There are
// no production infrastructure approvals for domain, contract, or policy crates.
pub(super) const PACKAGE_POLICIES: &[ExternalPackagePolicy] = &[
    ExternalPackagePolicy {
        name: "fleetiq-teltonika",
        normal: &["wit-bindgen"],
        development: NONE,
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-custom-protocol-example",
        normal: &["wit-bindgen"],
        development: NONE,
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-access",
        normal: NONE,
        development: &["tokio"],
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-analytics",
        normal: &["bigdecimal"],
        development: NONE,
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-domain",
        normal: &["bigdecimal"],
        development: NONE,
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-history",
        normal: NONE,
        development: &["tokio"],
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-identity",
        normal: NONE,
        development: &["tokio"],
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-ingestion",
        normal: &["sha2"],
        development: &["tokio"],
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-projection",
        normal: NONE,
        development: NONE,
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-live",
        normal: NONE,
        development: NONE,
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-metadata",
        normal: NONE,
        development: &["tokio"],
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-postgres",
        normal: &["serde_json", "bigdecimal", "sqlx"],
        development: &["tokio"],
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-protocol-host",
        normal: NONE,
        development: NONE,
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-protocol-sdk",
        normal: &["serde", "sha2"],
        development: NONE,
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-protocol-wasm",
        normal: &["rlimit", "serde_json", "sha2", "wasmtime"],
        development: &["wat", "wit-component", "wit-parser"],
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-semantics",
        normal: NONE,
        development: &["tokio"],
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-server",
        normal: &[
            "axum",
            "cookie",
            "futures-util",
            "getrandom",
            "openidconnect",
            "serde",
            "serde_json",
            "sha2",
            "sqlx",
            "subtle",
            "tokio",
            "url",
        ],
        development: &["jsonschema", "rcgen", "tokio", "tokio-rustls", "tower"],
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-telemetry",
        normal: NONE,
        development: &["bigdecimal"],
        build: NONE,
    },
    ExternalPackagePolicy {
        name: "fleetiq-xtask",
        normal: &[
            "cargo_metadata",
            "percent-encoding",
            "pulldown-cmark",
            "serde_json",
            "syn",
            "toml",
        ],
        development: NONE,
        build: NONE,
    },
];

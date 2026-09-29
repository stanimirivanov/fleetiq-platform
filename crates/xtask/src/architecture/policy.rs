use super::model::{CrateRole, DependencyClass, PackagePolicy, TemporaryException};

const NONE: &[&str] = &[];

pub(super) const PACKAGE_POLICIES: &[PackagePolicy] = &[
    PackagePolicy {
        name: "fleetiq-analytics",
        role: CrateRole::Policy,
        normal: &["fleetiq-domain"],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "fleetiq-application",
        role: CrateRole::Application,
        normal: &[
            "fleetiq-domain",
            "fleetiq-protocol-host",
            "fleetiq-protocol-sdk",
            "fleetiq-telemetry",
        ],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "fleetiq-domain",
        role: CrateRole::Domain,
        normal: NONE,
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "fleetiq-postgres",
        role: CrateRole::Adapter,
        normal: &["fleetiq-application", "fleetiq-domain", "fleetiq-telemetry"],
        development: &["fleetiq-protocol-host", "fleetiq-protocol-sdk"],
        build: NONE,
    },
    PackagePolicy {
        name: "fleetiq-protocol-host",
        role: CrateRole::Policy,
        normal: &["fleetiq-protocol-sdk", "fleetiq-telemetry"],
        development: &["fleetiq-domain"],
        build: NONE,
    },
    PackagePolicy {
        name: "fleetiq-protocol-sdk",
        role: CrateRole::Contract,
        normal: &["fleetiq-domain", "fleetiq-telemetry"],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "fleetiq-protocol-wasm",
        role: CrateRole::Adapter,
        normal: &[
            "fleetiq-domain",
            "fleetiq-protocol-host",
            "fleetiq-protocol-sdk",
            "fleetiq-telemetry",
        ],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "fleetiq-semantics",
        role: CrateRole::Policy,
        normal: &["fleetiq-domain", "fleetiq-telemetry"],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "fleetiq-server",
        role: CrateRole::CompositionRoot,
        normal: &["fleetiq-application", "fleetiq-domain", "fleetiq-postgres"],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "fleetiq-telemetry",
        role: CrateRole::Contract,
        normal: &["fleetiq-domain"],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "fleetiq-xtask",
        role: CrateRole::Tooling,
        normal: NONE,
        development: NONE,
        build: NONE,
    },
];

pub(super) const TEMPORARY_EXCEPTIONS: &[TemporaryException] = &[TemporaryException {
    package: "fleetiq-semantics",
    dependency: "fleetiq-application",
    class: DependencyClass::Normal,
    tracking: "ADR 0066 semantic read-port inversion",
    reason: "ADR 0003 currently adapts application repositories into semantic read views",
}];

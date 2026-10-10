# Security policy

## TL;DR

- Do not disclose suspected vulnerabilities in public issues, discussions, or
  pull requests.
- Prefer GitHub private vulnerability reporting.
- FleetIQ is pre-release; security fixes target the current default branch.
- Test only systems, devices, networks, and data you own or are authorized to
  assess.
- Treat protocol packages, telemetry, device replies, and customer asset data
  as security-sensitive inputs.

## Supported versions

FleetIQ has no supported production release. Security fixes target the current
default branch. Before the first release, this section must become an explicit
supported-version and end-of-support table.

The historical `fleetiq` and `fleetiq-ui` proof-of-concept repositories are not
supported production products.

## Reporting a vulnerability

Use the repository Security page's **Report a vulnerability** action when it is
available. If private vulnerability reporting is unavailable, request a private
contact through the repository owner without publishing component, exploit,
data, or reproduction details.

Include where applicable:

- affected revision, crate, deployment boundary, and configuration;
- impact and conditions required to reproduce it;
- a minimal proof of concept using synthetic data;
- redacted logs without credentials, identifiers, payloads, or customer data;
- known mitigations; and
- any intended disclosure date.

Do not retain, modify, or disclose data beyond what is necessary to demonstrate
an authorized issue safely.

## Triage and coordinated disclosure

Maintainers should acknowledge a private report, validate its scope and
severity, and agree on communication before disclosure. Response targets remain
best-effort until a staffed security process exists.

Reporter and maintainers should coordinate disclosure after a fix or effective
mitigation is available. Maintainers may publish a GitHub security advisory and
credit the reporter unless anonymity is requested.

## Security expectations for changes

Changes affecting authentication, authorization, tenant isolation, protocol
package admission, sandbox limits, device commands, sensitive telemetry,
credentials, retention, or external connectivity must document their threat
and failure model and receive focused review.

FleetIQ specifically requires:

- deny-by-default capabilities for untrusted protocol packages;
- explicit CPU, time, memory, stack, table, input, output, and collection
  bounds where untrusted processing can consume resources;
- host ownership of transport, identity, secrets, durable writes, and side
  effects;
- tenant-safe keys and references at persistence boundaries;
- redaction of wire payloads, credentials, and customer asset data from routine
  logs and errors; and
- authenticated, authorized, and auditable device commands before commands are
  enabled outside controlled development.

Before publication, run the pinned redacted scanner over reviewable files and
incoming commit history. Follow [harness protection](docs/development/harness-protection.md)
for exact commands, scope, exceptions, and handling a finding. Removing a secret
from the final tree does not remove exposure from Git history; rotate or revoke
any genuine credential and handle remediation privately. Scanner exceptions
apply only to reviewed synthetic fixtures with an exact scope, owner, reason,
and review trigger. A passing scan is not proof that every secret format or
runtime disclosure is covered.

See the [architecture](docs/architecture/README.md), [engineering standards](docs/development/engineering-standards.md),
and [contributor workflow](CONTRIBUTING.md).

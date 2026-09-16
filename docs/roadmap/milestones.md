# FleetIQ implementation milestones

## TL;DR

- Milestones are outcome-oriented and use `MNN - Outcome`.
- Each issue belongs to exactly one milestone and should fit one independently
  reviewable pull request.
- M01 records contracts; M02 and M03 establish product and ingestion
  foundations; M04 through M06 deliver semantic product value and data
  lifecycle; M07 through M09 qualify and release the system.
- GitHub owns live issue state; this document owns intended sequence and scope.

## Milestone index

| Milestone | Short message |
|:--|:--|
| M01 - Decisions and Contracts | Establish the vocabulary, boundaries, and versioned contracts that later implementation can rely on. |
| M02 - Product Foundation | Deliver tenant-owned metadata, identity, APIs, and the minimum operable backend foundation. |
| M03 - Extensible Device Ingestion | Accept real device traffic through secure, versioned, and recoverable protocol packages. |
| M04 - Semantic Twin and Live UI | Turn source observations into an explorable live asset graph and operator experience. |
| M05 - Analytics and Operations | Detect actionable conditions and operate ingestion and analytics predictably. |
| M06 - History, Archive, and Cost | Provide bounded historical access, retention, archival recovery, and cost controls. |
| M07 - Hardware Qualification | Prove supported hardware and protocols against repeatable laboratory and field evidence. |
| M08 - Production Readiness | Secure, observe, deploy, scale, back up, and recover the platform under defined objectives. |
| M09 - Customer Pilot and Release | Validate the product with a real customer workflow and release a supported first version. |

## Milestone boundaries

### M01 - Decisions and Contracts

Defines the product-neutral asset model, telemetry and semantic contracts,
protocol host and component boundaries, persistence decisions, contributor
harness, and ADR practice. Foundational code belongs here when its result is a
reusable contract rather than an operable product capability.

### M02 - Product Foundation

Adds the backend composition root, configuration, tenant and actor boundaries,
metadata application services, administrative APIs, authorization foundations,
audit semantics, and development deployment needed to operate the catalogue.

### M03 - Extensible Device Ingestion

Completes package artifact storage and activation, transport listeners, device
session routing, Teltonika as the first production package, custom-package
tooling, durable processing workers, replay, and ingestion observability.

### M04 - Semantic Twin and Live UI

Persists semantic property history, executes bindings, exposes graph and live
query APIs, and delivers the first web experience for assets, relationships,
telemetry state, provenance, and resnapshot recovery.

### M05 - Analytics and Operations

Runs versioned calculators and anomaly detection, persists outputs, defines
alert rules and delivery, and provides operational controls for failed or
lagging work.

### M06 - History, Archive, and Cost

Adds historical queries, downsampling, retention, object archival and restore,
bounded exports, storage accounting, and policies that keep tenant cost and
query load predictable.

### M07 - Hardware Qualification

Introduces captured synthetic and authorized device fixtures, protocol
conformance, network-failure scenarios, interoperability matrices, throughput
evidence, and repeatable qualification reports for supported hardware.

### M08 - Production Readiness

Completes production identity and authorization, threat modeling, secrets,
observability, SLOs, capacity and failure testing, deployment, staged rollout,
backup and restore, disaster recovery, privacy, and dependency review.

### M09 - Customer Pilot and Release

Configures one real customer asset model without hard-coding its industry,
operates an agreed pilot workflow, closes material findings, publishes support
and upgrade policy, and releases the first supported product version.

## Planning rules

- GitHub owns live issue state, labels, assignees, and milestone assignment.
- This file owns intended sequence until an issue is created.
- Every issue names exactly one milestone using its exact title.
- Published milestone numbers are never reused or renumbered.
- Move an issue only when its outcome dependency changes; update this roadmap
  when that changes the intended sequence.
- Split work that reveals multiple independently valuable or risky outcomes.
- A milestone may complete with deferred work only when its short message
  remains true and the deferral is recorded.

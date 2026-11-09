# Hardware qualification evidence contract

## TL;DR

M07 starts with one owner-selected Teltonika target. A support claim needs
matching laboratory and field results with retrievable original evidence.
Synthetic fixtures exercise the software and carry their own evidence class.

## Scope and ownership

[ADR 0160](../../docs/adr/0160-qualify-hardware-with-scoped-evidence.md) records
the decision. The platform owns the fixture and report vocabulary. Deployment
owns execution and matrix assembly. Consumers pin a reviewed contract revision
and evaluate reports against that revision.

A target tuple contains model, hardware revision, firmware, settings profile
revision and codec. Use an opaque profile identifier with an immutable revision
or a digest of a reviewed, sanitized profile. Where a target supports several
settings profiles or codecs, assess each combination independently. Coverage is
limited to the implemented TCP subset in the
[Teltonika package guide](../../packages/teltonika/README.md).

The owner supplies the following before the relevant run.

| Input | Required evidence |
| --- | --- |
| Target | Exact tuple and authorized physical device available to the operator |
| Ingress | Approved private path to the commissioned channel |
| Authorization | Named custodian and reviewer in a private authorization record, with capture scope and validity |
| Custody | Approved private store, access policy, retention limit and retrieval procedure |
| Laboratory | Frozen scenario list, expected observations and acceptable outcomes |
| Field | Authorized access, duration, operating conditions, sample coverage and allowed failures fixed before observation |

The laboratory tracer checks access and the collection path. Complete laboratory
and field campaigns are separate gates. The current target and access details
are pending owner input. Follow the [TCP ingestion guide](../../docs/operations/tcp-ingestion.md)
for listener and route prerequisites. An IMEI is a routing identifier.

## Fixture contract v1

A fixture entry binds a stable case identifier to an evidence class, codec,
content digest, bounded input and expected result. The digest is SHA-256 over
the exact referenced bytes. A reference resolves within the reviewed fixture
root or through the approved private custody procedure. Reject references that
escape their root. Check input size before parsing and apply the existing packet,
record and session limits before replay. The executable contract will specify
closed shapes and finite manifest, case and report limits.

| Evidence class | Provenance |
| --- | --- |
| Synthetic | Reviewed generator or hand-authored case, exact fixture digest and expected result |
| Authorized original | Exact target tuple, bounded capture scope, authorization and custody references, and digest of the original bytes |
| Synthetic derivative | Synthetic fixture digest, private parent evidence reference and reviewed transformation description |

Keep original captures immutable in private storage. Keep raw packets from real
devices, identifiers, locations, timestamps from captures, opaque IO and
credentials out of Git, CI artifacts and public diagnostics. Publish only
approved synthetic bytes and bounded, sanitized metadata. A derivative must
replace sensitive values and reseal the resulting frame. Review transformed
bytes before publication. A derivative can demonstrate regression coverage for
the new bytes. Original-device claims require original evidence.

Expected results come from an independently reviewed wire specification and
case oracle, with model-specific manufacturer references when a claim depends
on model-specific IO meaning. Keep expected results independent of the
decoder's own output. Cover wire fields, order, quality, timing and failure
outcomes where relevant. Store private expectations with the private evidence.

## Report contract v1

A report binds a run identifier and evidence class to the following information.

- Target tuple for physical runs, or a synthetic profile for automated runs.
- Contract revision, platform and deployment revisions, applicable image
  identities, admitted package digest, and fixture or capture references.
- Approved scenario and workload revisions, run window, environment and
  recorded resource limits.
- Every required case and attempt, its outcome, bounded reason and evidence
  reference. Include failed attempts and cleanup outcome.
- For measurements, sent, accepted and completed counts, sampling duration,
  throughput, ACK and completion latency, and the scope of resource readings.
  Record limits, warm-up, and any generator bottleneck.

An ACK establishes raw acceptance. Decoding, semantic storage and operator
visibility require their own observations. Reconcile recovery by accepted
message identity. A reconnect can produce a new identity for retransmitted
bytes. Historical replay is reply-free.

| Outcome | Meaning |
| --- | --- |
| Passed | Every required assertion was executed and met, and required evidence and cleanup are complete |
| Failed | A required assertion or cleanup check failed |
| Incomplete | The run began but execution, evidence or a prerequisite could not be verified |
| Not run | A planned case or campaign did not start |

Classify an observed failure as failed even if other evidence is incomplete.
Every missing case must be represented explicitly. Unknown fields, unsupported
versions and inconsistent or missing identities prevent acceptance. Reports
retain bounded metadata and safe reason codes. Authorization and custody
references require human review. A digest establishes byte identity when
resolved against the retained original.

## Support decisions and evolution

Qualify only the exact tuple covered by approved laboratory and field evidence
against the declared software, package and scenario revisions. Preserve
incomplete and failed rows in the interoperability matrix. Check access to
required originals at review time. Expired, withdrawn or unavailable evidence
blocks a current support claim. Retain the historical report with its original
scope and status.

Report throughput for the measured workload and environment. Introduce numeric
acceptance limits only after the owner approves the workload, resource envelope
and threshold. Hardware campaign completion and synthetic CI are separate
evidence classes.

Retain the published v1 contract while consumers depend on it. Review
compatibility before publishing changes. Use a new version for changes to
required fields or evidence meaning that existing consumers cannot interpret.
During rollback, revert dependent behavior before withdrawing its shared
contract.

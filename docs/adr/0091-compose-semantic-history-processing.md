# ADR 0091: Compose semantic resolution with durable history

## Status

Accepted, 3 October 2026.

Supersedes: none. Superseded by: none.

## Context

The semantic resolver attributes a decoded observation without writing, and
ADR 0090 provides an append-only history port. A caller still needs a stable
way to choose event time, resolve the corresponding bitemporal binding, and
append that exact interpretation. Independent timestamp choices can produce
a change whose event time differs from its binding effective time. A blind
retry after an ambiguous append can resolve against a later correction.

## Decision

`fleetiq-history` owns a one-observation application processor composing the
semantic resolver and history append port. The caller supplies the decoded
observation, recorded-time knowledge cutoff, and device-or-receipt event-time
basis. The selected timestamp is also the binding effective-time cutoff.
Missing selected device time fails before repository access. Unresolved
configuration returns a structured result that retains the observation.

Read failures retain the original observation, knowledge cutoff, and time
basis for equivalent re-resolution. Append
failures retain the complete property change; an explicit exact-change append
operation retries it without consulting current bindings. Successful append
reports whether it was new or already present. The PostgreSQL adapter remains
responsible for checking the candidate against durable source and binding
facts. This processor is an application operation, not a worker or transport.

## Consequences

Event and effective time stay aligned for this processing path. Callers must
choose the knowledge cutoff explicitly and retain unresolved outcomes for
later diagnosis or reprocessing. A lost append response can be retried with
the same interpretation identity even after a binding correction. Other
callers of the lower-level history port remain responsible for their own
timestamp and retry policy.

The processor does not schedule work, batch observations, select clock-trust
policy, resolve competing bindings, or publish live state. A production worker
must later supply those concerns and preserve the retry input across its own
durability boundary.

## Alternatives considered

Putting persistence orchestration in `fleetiq-semantics` would make read-only
policy depend on an effectful application port. Re-resolving after every
ambiguous append is unsafe when the known binding view can change. Accepting
separate event and binding effective times would require an additional
semantic policy and schema contract that this first processing path does not
need.

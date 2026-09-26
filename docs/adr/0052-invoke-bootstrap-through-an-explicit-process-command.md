# ADR 0052: Invoke bootstrap through an explicit process command

## Status

Accepted, 26 September 2026.

Supersedes: none. Superseded by: none.

## Context

[ADR 0051](0051-bootstrap-the-first-platform-operator-once.md) established the
atomic, one-time platform bootstrap application and persistence boundary. It is
not yet operable because the server process has no inbound adapter for it.

Bootstrap cannot be an ordinary HTTP route: before it succeeds, FleetIQ has no
internal authority that can authenticate and authorize the operation. Running
it automatically during every server startup would let routine deployment
configuration attempt to create root authority and would make operator intent
unclear across concurrent replicas.

The initial operator's external subject can be personally identifying. Passing
it as a command-line argument would also expose it in process listings and
common command-history workflows.

## Decision

The `fleetiq-server` executable supports three explicit process modes:

- no argument or `serve` starts the long-lived backend;
- `bootstrap-platform-operator` performs one bootstrap attempt and exits; and
- `help`, `--help`, or `-h` prints bounded usage without loading configuration.

The bootstrap command accepts no positional identity values. It reads exactly
these environment variables:

- `DATABASE_URL`;
- `FLEETIQ_BOOTSTRAP_ACTOR_ID`;
- `FLEETIQ_BOOTSTRAP_AUTHORITY_ID`; and
- `FLEETIQ_BOOTSTRAP_EXTERNAL_SUBJECT`.

Configuration is validated into domain types before database access. Debug
output redacts the database URL and external subject. Unknown commands and
additional arguments fail without echoing their values.

The one-shot command connects to PostgreSQL, applies the embedded migration
chain, records trusted host time, invokes `PlatformBootstrapService`, closes its
pool, and exits. It does not bind an HTTP listener. Its only success output is a
non-sensitive disposition distinguishing a new bootstrap from an exact retry.

No-argument startup remains `serve` for compatibility with the current
container and local deployment configuration.

## Consequences

An operator can initialize a clean installation with an explicit, auditable
one-shot action. The existing database singleton remains the concurrency and
idempotency authority, so simultaneously invoked commands cannot establish two
operators.

Deployment tooling must restrict who can set the bootstrap environment and run
the command. Environment injection is transport for validated identity values,
not proof of ordinary FleetIQ authorization. Operators should remove the
bootstrap variables after the command completes.

The command does not add an identity provider, validate login credentials,
create an HTTP administration API, or automate secret distribution. Those
remain separate capabilities.

## Alternatives considered

An unauthenticated HTTP bootstrap route was rejected because network exposure
would create a remote root-authority race during installation.

Automatic bootstrap during `serve` startup was rejected because normal replica
startup should never attempt to establish platform authority.

Command-line identity flags were rejected because process listings and shell
history commonly retain their values.

Adding a general CLI parsing dependency was rejected because the current
closed command vocabulary is small, accepts no inline values, and can be parsed
and tested without another runtime dependency.

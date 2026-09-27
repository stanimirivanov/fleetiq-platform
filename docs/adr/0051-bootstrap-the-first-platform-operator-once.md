# ADR 0051: Bootstrap the first platform operator once

## Status

Accepted, 26 September 2026.

Supersedes: none. Superseded by: ADR 0053 for the initial platform grant set.

## Context

FleetIQ can authorize and execute tenant provisioning, but a new installation
contains no internal actor, external-identity mapping, or platform grant. The
ordinary authorization path cannot create its own first authority because no
authenticated actor can pass platform policy yet. Manually inserting unrelated
rows would make initialization non-atomic and leave no durable explanation of
how the root authority was established.

Bootstrap itself cannot require an existing FleetIQ authorization grant. It
therefore needs a deliberately separate, one-time boundary whose eventual
inbound adapter can be restricted to an operator-controlled installation
workflow.

## Decision

`PlatformBootstrapService` accepts one validated internal actor identity, one
exact trusted external identity, and trusted host time. It performs no product
authentication or authorization. Callers must not expose it as an ordinary
business route.

One PostgreSQL transaction:

1. claims the installation-wide singleton row in
   `identity.platform_bootstrap`;
2. creates the internal actor;
3. maps the exact authority and opaque subject to that actor; and
4. grants only the current `tenant.provision` platform action.

The bootstrap row is inserted first with `ON CONFLICT DO NOTHING`. Deferred
foreign keys allow it to reference the actor and exact three-column external
identity mapping created later in the same transaction. The singleton key
serializes concurrent requests before dependent state commits. Any failed
insert or deferred reference check rolls back every bootstrap write.

After a uniqueness conflict, a separate statement reads the committed singleton
under PostgreSQL `READ COMMITTED`. Equal actor and external-identity fields are
an exact retry and return `AlreadyBootstrapped`; a different actor, authority,
or subject returns `AlreadyInitialized`. Retry time is not compared because the
first successful transaction owns the immutable audit timestamp and a later
operator invocation cannot reproduce it safely.

A trigger rejects update and deletion of bootstrap evidence. A composite
foreign key requires the recorded external identity to map to the same actor,
instead of independently proving only that both an actor and mapping exist.

## Consequences

A clean installation can establish one platform operator without direct
multi-row database seeding. Once an inbound bootstrap adapter is added, that
operator's authenticated external identity can resolve to the internal actor
and pass the normal `tenant.provision` policy.

No later request can replace the root bootstrap record or silently expand its
authority. New platform actions are not granted automatically. Ordinary actor,
external-identity, and platform-grant administration still need authorized,
audited commands.

An exact bootstrap retry reports the existing operation without recreating
current grant state. A later revocation therefore remains effective and cannot
be bypassed by replaying bootstrap input.

This slice provides the application and persistence capability but does not
choose a CLI, deployment job, secret format, or identity provider. Until a
controlled inbound adapter exists, the server does not invoke bootstrap.

## Alternatives considered

Seeding the actor, mapping, and grant independently was rejected because a
failure could leave partial authority and no atomic audit fact.

Running bootstrap automatically from environment variables on every server
startup was rejected because routine process configuration is not sufficient
proof to create root authority and concurrent replicas could obscure operator
intent.

Using a normal platform grant was rejected because it recreates the circular
dependency: an actor would need authority before the first authority exists.

Allowing multiple bootstrap records was rejected because bootstrap establishes
only the first trust root. Later authority changes belong to authenticated and
audited administration.

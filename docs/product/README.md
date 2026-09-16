# Product scope

## TL;DR

FleetIQ turns device telemetry into an explainable, tenant-owned digital twin
of fleets and equipment. The model is industry-neutral, protocols are
extensible, and semantic attribution is configured rather than compiled into
vendor decoders. The first product proves this path before expanding into a
public platform or industry-specific suite.

## Product intent

FleetIQ helps organizations operate physical assets whose telemetry arrives
through heterogeneous devices and protocols. Operators should see an asset and
its relevant subsystems, current state, history, anomalies, and alerts without
having to interpret vendor parameter numbers.

The core model is deliberately general. A customer may represent a truck,
trailer, and refrigeration unit; a locomotive, motors, sensors, and battery
management system; or another hierarchy. Customer-specific terminology and
schemas are configuration built on stable platform concepts.

## Core vocabulary

- A **tenant** owns configuration and data.
- An **asset** is a physical or logical thing with an internal FleetIQ identity
  and one versioned asset type.
- An **external identifier** is a typed value in an optional authority
  namespace, such as an IMEI, serial number, or operator number.
- An **asset relationship** is a typed, effective-dated edge between assets.
- A **communication device** is an asset capable of providing telemetry through
  stable endpoints.
- A **protocol package** interprets one vendor or custom wire protocol under a
  trusted host's controls.
- A **decoded observation** preserves what a protocol package reported and its
  provenance without deciding which business asset property it represents.
- A **signal binding** attributes a decoded source signal to a versioned
  property of an asset for an effective interval.
- An **asset property change** is normalized semantic telemetry suitable for
  history, live state, analytics, and alerts.
- A **calculator** derives semantic properties or events from other semantic
  data with pinned provenance.

## Initial users and outcomes

Fleet and equipment operators need to:

- register assets, devices, identifiers, and physical relationships;
- onboard supported or custom telemetry protocols safely;
- map device signals to the equipment they describe;
- inspect live and historical semantic state;
- detect anomalies and define alerts; and
- understand the source, timing, unit, decoder, and calculation behind a value.

Protocol authors need a constrained contract for framing and decoding traffic
without receiving database, network, credential, or arbitrary host access.

## Product principles

- **Asset-centric:** equipment and relationships are primary; devices are
  telemetry sources, not universal substitutes for vehicles or machines.
- **Semantic separation:** decoders report source observations; configuration
  attributes them to asset properties.
- **Explainable history:** event time, receipt time, recorded revisions,
  package releases, bindings, and calculator versions remain traceable.
- **Tenant ownership:** identity, configuration, and data are tenant-scoped at
  application and persistence boundaries.
- **Extensible with controls:** custom protocols and definitions are supported
  through versioned contracts and bounded execution.
- **Cost-aware evolution:** begin with a modular Rust application and
  PostgreSQL, then split or specialize storage only when measured needs justify
  operational cost.

## Deliberate boundaries

The initial product does not attempt to reproduce the breadth of a public
telemetry platform. A custom database engine, protocol virtual machine
language, REST-over-MQTT, marketplace, or separately deployed service for every
concept requires demonstrated product need.

Driver management, route optimization, maintenance planning, billing, broad
command execution, and detailed locomotive-specific schemas are deferred. The
model must allow them without pretending their requirements are already known.

See [current state](../development/current-state.md) for what is implemented and
[milestones](../roadmap/milestones.md) for the intended sequence.

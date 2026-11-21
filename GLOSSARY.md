# FleetIQ product language

## TL;DR

Canonical shared product terminology. Repository-local vocabulary extends these meanings without redefining them.

## Language

**Tenant**: An ownership and authorization scope for FleetIQ resources and activity.

**Actor**: An identity performing an action in FleetIQ; authority comes from explicit grants.

**Grant**: Explicit authority to perform an action within a scope. Membership, platform administration, read, manage and acknowledgement are distinct authorities.

**Asset**: A physical or logical thing managed by FleetIQ, whose meaning is supplied by its asset type. _Avoid_: vehicle as a synonym for every asset.

**Asset type**: A versioned definition of an asset category and its supported properties; vehicle is one possible category.

**Communication device**: Communication capability attached to an asset using that asset’s identity. A tracker and the truck it observes can be separate assets. _Avoid_: equating every asset with a device.

**Device endpoint**: A declared observation source on a communication device.

**Wire identity**: An identifier presented through a device protocol, such as an IMEI. Possession or presentation alone does not establish authenticated physical identity.

**Fleet**: The tenant-scoped collection of assets in the current product vocabulary; it is not a separately managed membership entity.

**Fleet view**: An inventory and telemetry presentation of a tenant’s assets. Simulator fleet has a separate, explicitly synthetic meaning.

**Property definition**: An exact versioned meaning, value kind and canonical unit for an asset property. Equal names across versions or tenants do not establish equivalence.

**Asset relationship**: A directed, typed relationship between assets, distinct from assigning a signal to a property.

**Source signal**: An identified observation emitted from a device endpoint; protocol meaning precedes business attribution.

**Signal binding**: An effective-dated attribution of a device/endpoint/source signal to an asset and exact property-definition version.

**Decoded observation**: A protocol-neutral source fact retaining value, quality, time and provenance before business attribution.

**Semantic interpretation**: An attribution of a decoded observation using selected semantic knowledge and time policy.

**Property change**: An attributed property update candidate; its value need not differ from the preceding value.

**Current property value**: The interpretation selected as current for an asset property. _Avoid_: using unqualified reading for both source observations and current values.

**Event time**: The time an observation is treated as occurring under its selected time basis; it is not proof of clock accuracy.

**Receipt time**: The time a host received the source message.

**Effective time**: The time at which configuration applies to the domain event being interpreted.

**Recorded time**: The time configuration or evidence became known to the system.

**Knowledge cutoff**: The recorded-time boundary limiting which configuration knowledge participates in a selection.

**Quality**: The good, uncertain or invalid assessment accompanying evidence, independent of its age.

**Freshness**: The fresh, stale or future classification of evidence relative to an evaluation time.

**Location validity**: Whether coordinate evidence satisfies the required value and coherence rules; freshness is a separate assessment.

**Connection liveness**: Evidence of a connection’s current activity. Recent telemetry, stored receipts and worker startup alone do not establish liveness or overall health.

**Protocol acknowledgement**: A peer’s protocol-level acknowledgement of records, distinct from subsequent processing or visibility.

**Durable acceptance**: Retention of an accepted message under the receiving system’s durability contract.

**Processing evidence**: Evidence of a named stage: semantically processed, projected or browser-visible. Each stage requires its own observation. _Avoid_: delivered without a named boundary.

**Registered-identifier routing**: Selection of a registered device route using identifier evidence; this is distinct from cryptographic peer authentication.

**Published version**: An exact retained definition or release made available for selection; publication alone does not activate it.

**Configuration revision**: An immutable version of configuration content.

**Active selection**: The revision or release currently selected for use.

**Command receipt**: Evidence of a recorded command outcome, potentially historical; it does not establish the current selection.

**Retry**: Another attempt at the same work, retaining that work’s specified inputs and identity.

**Protocol replay**: Processing retained accepted messages through a selected protocol decoder.

**Reinterpretation**: Applying explicitly selected semantic knowledge to retained observations.

**Calculator output**: Versioned derived evidence computed from semantic properties, distinct from a decoded observation.

**Alert**: An observed condition governed by a selected rule revision and its lifecycle.

**Alert acknowledgement**: An actor’s acknowledgement of a specific alert transition; it does not resolve the condition or suppress notifications.

**Alert resolution**: The lifecycle transition establishing that the firing condition has resolved.

**Notification suppression**: A policy that inhibits notifications; this term does not assert that a suppression feature is implemented.

**Receiver delivery**: Evidence that a notification reached its intended receiver, distinct from a router accepting the request.

**Historical evidence**: Retained interpretations and provenance concerning past observations.

**Archived evidence**: Historical evidence retained in a published archive representation.

**Restored selection**: A selected portion of archived evidence made available for bounded historical access.

**Numeric summary**: Derived numeric evidence over a stated selection and interval, distinct from raw observations.

**Coverage**: An explicit account of included evidence and known limitations; restored or summarized does not mean complete.

# Architecture

FleetIQ is an asset-centric platform. Its core model supports multiple industries
and treats locomotives as one prospective configuration rather than a universal
domain assumption.

The principal boundaries are:

1. A protocol layer turns bytes from an identified communication device into
   source observations while preserving provenance.
2. A semantic layer binds source observations to typed properties of physical
   assets for an effective time interval.
3. A telemetry layer stores current and historical observations and supplies
   bounded queries.
4. An application layer implements asset management, calculations, alerts,
   commands, and user-facing workflows.
5. Adapters integrate transports, persistence, brokers, identity providers, and
   object storage without leaking them into the domain model.

The initial runtime topology and technology choices remain subject to focused
ADRs and measured prototypes. See the source redesign documents until their
accepted parts are migrated into this repository.

# Observed locations and fleet coverage

## TL;DR

GET the authorized tenant fleet with explicitly selected WGS84 coordinate
properties, exact versions and encoding. The response keeps unlocated assets
and reports whole-tenant inventory/projection coverage. It never reports device
connectivity, asset health or inferred last-good positions.

## Configure and read

1. Publish numeric coordinate definitions and install both on the relevant asset
   types. Bind the exact decoder source signals to those versions. The mapping
   declares WGS84 latitude/longitude semantics; other datums need an explicit
   upstream conversion rather than relabelling degrees.
2. Use `encoding=e7` for unitless signed integer degrees times ten million.
   The initial Teltonika GPS E7 signals already preserve these integers. Use
   `encoding=degrees` only when both definitions and source values use `deg`.
   Decimal coordinates preserve exact strings without floating-point rounding.
3. Run the existing semantic and current-state workers. This read starts no
   worker and an empty projection does not imply an empty upstream queue.
4. Authenticate with both tenant metadata.read and telemetry.read. Neither action
   implies the other. Browser clients use their existing same-origin session.
5. GET the exact query below. Preserve every mapping parameter when following
   `next_after`; it is an exclusive asset key, not authorization or a stable
   cross-page database snapshot.

```text
/api/v1/tenants/tenant-a/fleet?latitude_property=position.latitude&latitude_version=1&longitude_property=position.longitude&longitude_version=1&encoding=e7&limit=50
```

Both definitions must exist; otherwise the request returns 404. Unknown query
fields, invalid versions/limits/encoding or repeated axis identity return 400.
Missing grants return 403 before mapping discovery. Database failures/deadlines
return safe 500, never an empty success.

## Interpret the response

The mapping echo is read configuration, not saved tenant configuration. All
counts and rows share one repeatable view at generation/revision/pending. Counts
cover the entire tenant, even on an empty cursor suffix. Communication devices
are capabilities on assets; counts are not mutually exclusive fleet categories.
Mapped assets declare both selected versions; projected assets have at least one
current property of any kind. Neither count establishes usable locations.

| Location status | Meaning |
| --- | --- |
| unmapped | Installed type lacks at least one exact selected property version. |
| incompatible_mapping | Definition kind or unit disagrees with the encoding. |
| missing | At least one mapped current value is absent. |
| incoherent | Source group/device time is different, missing or ambiguous. |
| invalid_coordinates | Value/unit/range or 128-character coordinate budget is invalid. |
| invalid_quality | A latest candidate is invalid; no older good value is substituted. |
| observed | Both exact source facts form a supported bounded point. |

Observed points retain separate quality and freshness plus both original history
interpretations. Follow their existing interpretation endpoints for immutable
attribution. Uncertain does not become good because a value is fresh. Stale and
future points remain labelled evidence; the renderer must not treat them as
current healthy/online assets. Quality does not establish physical GPS accuracy.

Axes must be distinct source outputs/signals in one message/package/device/
endpoint with the same device-recorded timestamp and device time basis. Repeated
signal/timestamp groups inside a packet are rejected instead of mixing fixes.
Receipt-only coordinates remain unavailable. This deliberately omits ambiguous
records until richer decoder record identity is available.

## Bounds and qualification

Each page returns at most 100 assets and hydrates at most 200 source candidates
in fixed-count set-based queries. Statements have 2s deadlines and the read has
an 8s whole-operation deadline. Full tenant aggregates still scale with inventory;
timeout is a failure, not partial totals. Tenant writes/projector commits may
change later pages. No new schema or grant is introduced; existing runtime roles
need the current projection, history/source and metadata SELECT privileges.

Run focused projection tests locally; PostgreSQL, HTTP, contract and complete
Rust gates run on CI. Synthetic fixtures cover exact coordinates, ambiguity,
unsupported mapping, bounded pages, empty suffix totals and independent grants.
D06, full-profile infrastructure and hardware qualification remain deferred.

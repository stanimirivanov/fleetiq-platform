# Retained-history alert selection measurements

## TL;DR

Run the PostgreSQL lifecycle measurement before changing the selector or increasing
worker throughput. The fixture exercises the production SQL function at increasing
retained volumes and checks late-arrival correctness. Its baseline shows growing
receipt-prefix cost. Timings are observations tied to the workload and host.

## Reproduce

Use the repository-pinned Rust and Nextest tools and fully migrated PostgreSQL
18.6. Set DATABASE_URL to a disposable test service with database-creation authority.
SQLx creates an isolated test database. Run on an otherwise idle host and record
database placement, CPU/memory limits, PostgreSQL settings and toolchain versions
alongside the report.

```bash
cargo nextest run -p fleetiq-postgres --test alert_lifecycle --locked --profile ci -E 'test(measured_retained_history_selection)' --success-output immediate
```

The ordinary workspace suite also runs this fixture. Its reviewed Nextest
override reserves all eight global slots and the PostgreSQL group. This isolates
suite work while leaving host scheduling and external load under operator control.

## Workload and evidence

One active rule selects a fixed calculator release, scope, target and output.
The fixture grows bulk historical evaluation/receipt pairs through 0, 256, 4,096
and 32,768 rows. Each stage adds the same number of unconsumed evaluations from
another scope. Bulk historical receipts are seeded directly through schema
constraints to avoid timing fixture construction as lifecycle processing.

Earlier stages retain their consumed scheduled probes. The report records actual
receipts_before alongside the bulk-history and other-scope counts. Each stage
runs ANALYZE, then measures an exhausted selector and one pending scheduled item.
Five client latency samples include SQL round-trip time. Plan timing is a
separate EXPLAIN ANALYZE invocation. Setup and ANALYZE are outside those timers.
Whole idle and consuming worker turns are recorded separately and include
candidate discovery, locking, the repeated selector and receipt/lifecycle writes.

Each selector observation uses a repeatable-read read-only transaction and the
existing diagnostic two-second statement deadline. Worker turns have the existing
fifteen-second outer deadline. Assertions gate selection correctness, completed
scheduled-work provenance and unchanged receipt/transition/intent counts after
observations. Finally, two late commits with equal earlier event ends are selected
in C-collated ID order and receive out_of_order receipts without new transitions
or notification intents.

The fixture writes target/alert-selection-measurements/selection-<pid>.json.
Writing the report is part of test success. CI retains it with Rust evidence for
14 days. The report contains version, synthetic counts, min/median/max latency,
worker turn times, root shared/temp buffer totals and plan node types, rows, loops
and filter-removal counts. Raw SQL, predicates and connection configuration are
excluded by a tested projection. Root buffers already include child usage.
Node row counts are per-loop averages, so inspect loops when comparing plans.
See [PostgreSQL EXPLAIN interpretation](https://www.postgresql.org/docs/18/using-explain.html).

## Observed baseline

Measured on 5 November 2026 with PostgreSQL 18.6 in a local Linux Docker container,
reached over loopback, Rust 1.99.1 and Nextest 0.9.145. The container had no explicit
CPU or memory cap. This fresh synthetic dataset was warm after inserts and
ANALYZE. Host load and production storage behavior were not calibrated.

All times below are milliseconds. Selector columns show the median of five
samples. Worker columns show one complete turn.

| Bulk history | Receipts before | Exhausted selector | Pending selector | Idle worker | Consuming worker |
|:--|:--|:--|:--|:--|:--|
| 0 | 0 | 0.176 | 0.214 | 2.899 | 7.302 |
| 256 | 257 | 1.288 | 1.307 | 3.226 | 4.312 |
| 4096 | 4098 | 11.064 | 10.561 | 45.495 | 50.615 |
| 32768 | 32771 | 121.413 | 114.947 | 500.361 | 685.185 |

The largest stage exposes history-dependent selection and repeated worker cost.
Use the JSON plan and buffer evidence to assess a future optimization. Preserve
late discovery and equal-end ordering when comparing alternatives. There is no
portable timing threshold, cold-cache claim, multi-rule fairness measurement,
production capacity claim or selector/index change in this fixture.

# Run one bounded calculator evaluation

## TL;DR

After migrations and trusted declaration registration, run
`cargo run -p fleetiq-server -- calculate-once` with private `DATABASE_URL` and
`FLEETIQ_CALCULATOR_REQUEST_FILE` pointing to at most 4096 bytes of strict JSON.
No inline arguments, HTTP, startup execution or scheduler is involved.

```json
{"tenant":"tenant-a","scope":"asset-a","calculator":"mean","version":"1","evaluation":"evaluation-1","configuration":"config-1","target":"result","start":0,"end":3000,"finalize":3000,"known":3000,"evaluated":3500,"minimum":1}
```

Times are signed Unix milliseconds. Window/finalization/cutoff use PR1 bounds;
processing time must not precede cutoff. Supply these explicitly and retain the
exact request file on retries. The exact release must be registered, scope and
target must belong to the tenant, and the target type must support the registered
output property. Unknown fields and unfinalized/invalid requests fail closed.

Preparation selects only immutable semantic history for the declared scope and
input property/version in one SQL statement snapshot. It captures exact references,
usable values, quality usability and output definition and commits before computing. At most 1024
inputs are accepted; a 1025-row sentinel rejects overflow without storing a partial
selection. Retrying a tenant/evaluation ID reads its saved preparation first and
rejects changed request content. Late/backfilled facts cannot change that selection.
A new evaluation ID is required to calculate a later selection.

The reviewed `window-mean-v1` implementation adds numeric values exactly, then
truncates the quotient toward zero to 18 decimal places. No binary floating-point
conversion, library-default precision or implicit unit conversion is used. Numeric
coefficients are limited to 1024 characters and scales to +/-256. Arithmetic resource excess
persists a bounded failed outcome. Values exceeding the 4096-character capture
budget are marked unusable without loading unbounded numeric material. Empty,
below-minimum or unusable-quality inputs
persist insufficient evidence and no value. Only good numeric source samples count;
invalid quality is retained as unusable evidence rather than silently filtered out.

Terminal storage remains the atomic PR3 envelope. Storage timeout/failure may follow
a successful commit; retry the original file to reuse preparation and output identity.
Changing processing time, parameters, target, scope or release under the same ID is
rejected. This is trusted operator functionality, not tenant HTTP authorization.
The command checks schema, sets five-second statement deadlines and a 15-second
turn deadline, and never prints raw errors, values, file paths or credentials.

Apply migration 0040 before deploying this command. Its table is additive and
immutable. Roll back by stopping command use and retaining preparations/terminal
facts. Background scheduling, recursive calculated inputs and anomaly detection
remain later slices. See [evaluation storage](../development/calculator-evaluation-storage.md).

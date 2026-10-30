# Register a first-party calculator declaration

## TL;DR

Apply migrations first, then run `cargo run -p fleetiq-server -- register-calculator`
from a trusted operator environment. It checks schema, registers immutable content
and exits. It does not schedule, activate or execute calculators.

Set `DATABASE_URL` using the normal private environment mechanism and
`FLEETIQ_CALCULATOR_DEFINITION_FILE` to a UTF-8 JSON file of at most 4096 bytes:

```json
{"tenant":"tenant-a","calculator":"temperature-mean","version":"1","implementation":"window-mean-v1","input":"temperature","input_version":1,"output":"temperature.mean","output_version":1}
```

Both exact property definitions must already exist inside the selected tenant.
Input must be integer or decimal; output decimal; canonical units must match.
Unknown fields, implementation IDs, zero versions and invalid identifiers fail.
The first-party ABI is a declaration contract only until the later runner slice.
It supports one input role and one mean output role; future configuration uses
PR1 window/lateness bounds (seven days each), 1024 inputs maximum and a minimum
sample count between 1 and 1024. No free-form expressions or uploaded code exist.

The tenant/release key is immutable. Exact retries succeed, including concurrent
retries; changing implementation or either property reference conflicts. Correct
mistakes by registering a new version. The database rejects mutation and foreign
or incompatible references even for direct SQL writers.

This command trusts the operator's database access, like migration/bootstrap;
it is not a browser authorization path. Use a limited registry-writing database
role after separately approved migration. Errors never print connection strings,
JSON content or raw database diagnostics. No HTTP endpoint or automatic grants
are introduced. Roll back the process code by stopping use of this command;
retain registered facts and the additive schema for later compatible consumers.

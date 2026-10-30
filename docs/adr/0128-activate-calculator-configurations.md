# ADR 0128: Activate retained calculator configuration revisions

## Status

Proposed, 30 October 2026.

Supersedes: none. Superseded by: none.

## Context

The trusted scheduling installation in [ADR 0126](0126-schedule-fenced-calculator-work.md)
needs an authorized configuration boundary. Changing parameters must not mutate
existing requests or race a worker into claiming newly disabled work.

## Decision

Expose exact immutable configuration revision creation/reads and optimistic
activation state under `/api/v1/tenants/{tenant_id}/calculator-configurations`.
Creation requires the next positive u32 revision, or identical same-author retry.
Activation has a separate u32 generation and selects a retained revision or null.
Same-author/content retries return the original receipt even after later changes,
without restoring earlier state. Actor and database-clock receipt time are retained.

Configuration writes require independently delegated `analytics.manage`; reads
require `metadata.read`. No other grant implies this capability and no migration
upgrades grants. Existing listener authentication and browser Origin/CSRF guards
apply. Database write transactions recheck and lock active actor, tenant and exact
grant before the head, ordering revocation against mutation.

Each revision pins an admitted first-party release, semantic scope, compatible
output target and bounded fixed-window parameters. Execution identities derive
from tenant/configuration/revision. Creating a revision never changes the selected
revision. Its deliberate first_start controls historical catch-up when selected.

Activation serializes on the same configuration head locked by enumeration and
claim selection. Deactivation pauses future enumeration and unclaimed work for
that revision; issued claims may finish under their original lease and evidence.
Selecting another revision pauses old pending work, preserving it for a later
explicit selection. Reactivation resumes the original cursor and requests.

## Consequences

Migration 0043 adds retained revisions and activation receipts; it imports each
existing trusted schedule as inactive revision 1 with its cursor/work identity
unchanged. Unknown original authors remain null. Import time records migration
receipt, not an invented installation time. Operators must explicitly activate
these configurations. The transitional worker schedule-file option is rejected;
protected configuration commands replace installation through that file.

The closed action vocabulary expands to six; clients must regenerate before the
new action is delegated. No dependencies or automatic grants are added. Upgrade
all checked-schema workers, then delegate and activate deliberately. Rollback
stops affected workers and retains schema/history; older binaries are incompatible
with the expanded schema/vocabulary. Full scheduling recovery controls remain PR18.

## Alternatives considered

A mutable parameter row could reinterpret existing work. Independent enable flags
without shared locking race new claims. Canceling issued work at deactivation
would discard valid pinned computations and entangle operator control with lease
recovery. General cron expressions, arbitrary code and UI authoring remain out of scope.

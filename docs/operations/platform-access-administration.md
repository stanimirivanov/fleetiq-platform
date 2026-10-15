# Platform-access administration

## TL;DR

Use these routes only as a currently authenticated actor holding exact
`platform.access.manage`. Delegate to an active operator and verify their access
before revoking your own grant. Browser writes require the configured Origin and
`X-FleetIQ-CSRF: 1`. Keep command IDs stable after uncertain results.

## Authentication and exact authority

The server mounts the routes only when browser identity or workload identity is
configured. Browser clients use existing same-origin sessions; never put a workload
credential in browser code. All operations recheck active actor identity and exact
current authority. Tenant `access.manage`, platform `identity.manage` and other
actions cannot authorize this API. No new configuration or environment variable
is needed.

The action vocabulary is `identity.manage`, `platform.access.manage`,
`tenant.lifecycle.manage`, and `tenant.provision`. These grants are independent;
there are no implicit roles or inherited actions.

## Grant and revoke

POST `/api/v1/admin/actors/{actor_id}/platform-access-changes` with JSON:

```json
{"change_id":"grant-operator-b-001","kind":"grant","action":"platform.access.manage"}
```

Use a new validated command ID for each intended change, and use `revoke` for
ensure-absent. The target comes from the path; the server derives administrator
and receipt time. Unknown fields, unsupported actions and bodies over 16 KiB
return 400. Missing actors return 404.

Success is HTTP 200 with `outcome` (`recorded` or `already_recorded`) and the
original `state_changed` boolean. A fresh command for already satisfied state
records a no-change fact. An exact retry retains original time and effect without
reapplying old state. Changed content or administrator under the same ID conflicts.
A different identity resolving to the same active actor is still the same
administrator, subject to fresh current authority.

Revoking the final active platform-access administrator returns 409 and commits
neither grant removal nor audit. An inactive actor's retained grant does not
satisfy this protection. Activate and verify the successor before retiring access.

## Inspect current grants and immutable audit

GET `/api/v1/admin/actors/{actor_id}/platform-grants?limit=50` returns
`grants: [{action}]` and nullable `next_after`. Pass that exact action as
`after_action` to continue in ascending action-name order. The referenced grant
can be revoked between pages. Missing actors and actors with no grants return an
empty terminal page. Inactive actors retain grants but cannot exercise them.

GET `/api/v1/admin/platform-access-changes?limit=50` returns `changes` and nullable
`next_before`. Each fact includes command, trusted administrator, target, exact
action, grant/revoke kind, original recorded time, and original effect. Continue
using both `before_recorded_at_ms` and `before_change_id`, or neither. History is
newest first with command identity breaking timestamp ties.

Both page limits are 1–100, default 50. Unknown, duplicate or incomplete query
fields return 400. Cursors are traversal positions, not snapshots or permission.
Bootstrap grants have separate evidence, so administration history alone cannot
reconstruct every current grant.

## Failure, recovery and rollback

All outcomes are no-store. 401 means authentication no longer resolves; 403 means
current authority or browser mutation protection failed; 409 means collision or
lockout protection; 500 means a bounded internal failure. Problems omit SQL,
constraint names and credentials. Do not record credentials or request bodies
in infrastructure logs.

After timeout or lost response, retry the same command as the same actor with
current authority. Self-revocation may have succeeded and make the retry return
403. Ask another authorized administrator to inspect audit and current grants;
do not issue a new grant merely to guess the old result. Browser sessions do not
cache platform authorization. Previously authorized in-flight operations can
finish, and this API does not recall their evidence.

Rollback can remove additive routes while retaining grants and immutable audit;
keep the active-administrator correction. This slice does not qualify a production
provider, edge deployment, tenant-access API or administration UI. See
[ADR 0102](../adr/0102-expose-platform-access-administration-over-http.md).

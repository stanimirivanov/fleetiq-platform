# Actor administration

## TL;DR

An authenticated listener exposes actor provisioning, lifecycle commands, the
actor catalogue, and lifecycle audit. Every request requires the exact current
platform identity.manage grant. Browser commands require the configured Origin
and X-FleetIQ-CSRF: 1. Keep command IDs and business fields unchanged on retries.

## Prerequisites

Configure either [browser identity](browser-identity.md) under one HTTPS origin
or the workload bearer described in the [asset query procedure](asset-query.md).
Bootstrap the first operator using the controlled process command. No HTTP
bootstrap endpoint exists. Unconfigured listeners do not mount administration.

A provisioned actor receives no tenant or platform grants. Its authority/subject
must match the trusted authentication provider's exact identity. Provisioning
does not create a provider user or send an invitation.

## Operations

| Method and route | Result |
|:--|:--|
| GET /api/v1/admin/actors | Current active and deactivated actors, ascending actor ID |
| POST /api/v1/admin/actors | Atomic actor, initial exact external identity, and provisioning audit |
| POST /api/v1/admin/actors/{actor_id}/lifecycle | Audited activation or deactivation |
| GET /api/v1/admin/actor-lifecycle-changes | Immutable lifecycle history, newest first |

Example provisioning JSON:

```json
{
  "provisioning_id": "provision-operator-2",
  "actor_id": "operator-2",
  "external_identity": {
    "authority_id": "workforce",
    "subject": "provider-subject-2"
  }
}
```

Example lifecycle JSON:

```json
{
  "change_id": "deactivate-operator-2",
  "state": "deactivated"
}
```

Send application/json. Bodies are bounded to 16 KiB and reject unknown fields,
including caller-selected administrator or recorded-time fields. Browser commands
send credentials through the existing HttpOnly session cookie, one Origin equal
to the configured public origin, and one X-FleetIQ-CSRF header with value 1.
Do not send Authorization in browser mode. Workload mode uses its configured
bearer; it does not need browser mutation headers.

## Retries and conflicts

Reuse the command ID and complete business payload after an uncertain response.
The first committed transaction owns the host receipt time. Fresh authentication
and exact authority are required on every attempt. Provisioning returns 201 with
outcome provisioned initially and 200 with already_provisioned on exact retry.
Lifecycle returns 200 with outcome recorded or already_recorded and the original
state_changed value. That value describes the command's original effect, not
the actor's current state.

A later lifecycle transition is never undone by retrying an earlier command.
A new command ID requesting the already-current state records a no-change audit.
Changing command content or administrator under an existing ID returns 409.
A different command cannot take an existing actor or exact external identity.

## Paging

Both catalogues accept limit in 1..100, default 50. Actor pages accept an exclusive
after actor ID and return next_after or null. Audit pages return next_before
containing recorded_at_ms and change_id, or null. Supply both as
before_recorded_at_ms and before_change_id on the following audit request.
Keep their exact values; do not derive a cursor from local time.

Each page reads current committed state independently. Concurrent changes can
affect traversal; cursors do not grant access or establish a snapshot. Unknown
query fields, duplicate parameters, invalid bounds, and incomplete audit cursors
return 400.

## Failure and recovery

All outcomes are no-store. Missing authentication is 401; missing exact authority
or failed browser mutation protection is 403. A missing lifecycle target is 404.
Conflicting identities and attempts to deactivate the last active identity or
platform-access administrator are 409. Unsupported methods return 405 with Allow.
Malformed input is 400; unexpected storage failure is a redacted 500.

Provisioning and lifecycle audit failures roll back the complete transaction.
Investigate server/database health after 500, then retry the unchanged command;
do not compensate with direct SQL edits or a new identity after an uncertain result.
Deactivation retains mappings, grants, and history. Reactivation restores access
from retained configuration; remove grants or mappings separately when permanent
removal is intended. Deactivation affects the next authentication or policy check,
while accepted in-flight work may complete.

## Deployment and rollback

Use the existing listener and database configuration; no migration or additional
service is needed. The [modular OpenAPI contract](../../contracts/http/README.md)
is authoritative for payloads and statuses. Deploy API consumers only after the
contract reaches the backend main branch. Rolling back removes this additive
HTTP surface without deleting actors or audit facts. UI administration,
external-identity replacement, grant management, and tenant onboarding are later
implementation slices.

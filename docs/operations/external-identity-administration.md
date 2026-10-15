# External-identity administration

## TL;DR

Use exact current platform identity.manage authority to inspect mappings, attach
a verified replacement, and explicitly detach the retired mapping. Preserve exact
subject text. Reuse command IDs and business fields after uncertain results.
Never log request queries, bodies, response bodies, or opaque identity subjects.

## Prerequisites and trust

Configure the existing [browser session](browser-identity.md) or workload mode;
an unauthenticated listener has no administration routes. Use
[actor administration](actor-administration.md) to provision or retire an actor.
Provider users must already exist. Mapping administration creates no provider
account, invitation, tenant grant, or platform grant. A mapping does not configure
a new provider: its authority must match the selected listener's trusted
authentication configuration before that login can work.

This authority can change login identities across actors. Treat its delegation
as privileged identity administration. Neither tenant permissions nor other
platform actions substitute. Each request evaluates fresh authentication and the
exact platform grant; a catalogue result is never permission to make a later change.

## Operations

| Method and route | Result |
|:--|:--|
| GET /api/v1/admin/actors/{actor_id}/external-identities | Current exact mappings for one selected actor |
| POST /api/v1/admin/actors/{actor_id}/external-identity-changes | Atomic attach/detach and immutable audit evidence |
| GET /api/v1/admin/external-identity-changes | Bounded platform-wide change history |

Example command body:

```json
{
  "change_id": "attach-recovery",
  "kind": "attach",
  "external_identity": {
    "authority_id": "recovery",
    "subject": " Subject "
  }
}
```

Use application/json. Bodies are bounded to 16 KiB and reject unknown fields,
including administrator, target actor, and recorded-time overrides. The target
comes from the path; the administrator and receipt time come from trusted host
evidence. Authority and subject are case-sensitive. Spaces within a valid opaque
subject are meaningful: never trim or normalize them.

Browser mutations require the existing HttpOnly cookie, one Origin equal to the
configured public origin, and one X-FleetIQ-CSRF: 1 header. Do not send Authorization
in browser mode. Workload mode uses its configured bearer and requires no browser
mutation headers. No cross-origin permissions are added.

## Safe replacement and retry

1. Inspect the selected actor's current mappings.
2. Attach the new exact identity using a new command ID.
3. Verify that the replacement identity authenticates through the intended
   provider. Before changing your own login, verify a recovery session with the
   required identity.manage authority.
4. Detach the retired identity using another command ID.
5. Review current mappings and immutable change history.

The backend prevents removal of a final mapping; it cannot prove provider
availability or ownership of a newly registered account. An identity belonging
to another actor conflicts for both attach and detach. It is never moved
implicitly. Ordinary replacement retains stable actors, grants, and historical
provisioning evidence.

After a lost response, repeat the same command ID, target, kind, authority, and
subject. Fresh authentication and current exact authority still apply. A 200
response contains outcome recorded or already_recorded and the original
state_changed boolean. A new command whose state is already satisfied records
false. An old retry never reattaches or removes a mapping changed by later commands.

Detaching the mapping used by the current session makes its next request fail
authentication. Use the verified replacement identity for further work; it must
resolve to the same administrator for exact retry. Accepted in-flight requests
are not recalled. After an uncertain result, inspect current mappings and audit
with a working administrator rather than editing SQL or inventing a new command.

## Paging and sensitive cursors

Both reads accept limit in 1..100, default 50. Current mapping pages return
identities in ascending (authority_id, subject) order and a next_after tuple or
null. Send both tuple fields as after_authority_id and after_subject on the next
request, using the HTTP client's standard query encoder. A detached cursor
mapping need not still exist. Another actor has its own independent traversal.
Missing actors and actors with no mappings both return an empty terminal page.

Audit pages return changes newest first by (recorded_at_ms, change_id).
Continue using both before_recorded_at_ms and before_change_id from next_before.
Initial bootstrap and provisioning mappings have separate evidence and do not
necessarily appear in this attach/detach history. Do not reconstruct current
mappings from audit pages.

Each page reads committed state independently. Invalid bounds, unknown or
duplicate parameters, and incomplete cursor pairs return 400. Cursors contain
sensitive subject data and grant no authority. URL encoding is not encryption.
Configure edge access logs to record route templates and statuses, omitting query
strings and request/response bodies. Keep subjects and cursors out of screenshots,
error reports, telemetry, and routine application diagnostics.

## Failure, rollout, and rollback

Every outcome is no-store. Invalid input is 400; missing authentication is 401;
missing exact authority or browser mutation protection is 403; a missing command
target is 404. Identity collisions, implicit rebinding, and final-identity removal
are generic 409 problems. Unsupported methods return 405 with Allow; unexpected
storage or audit failure returns a redacted 500 and rolls back the full mutation.

After a 500, investigate service/database health and retry the unchanged command.
No new configuration, migration, provider connection, or service is required.
Consumers pin the merged [modular contract](../../contracts/http/README.md).
Rollback removes the additive routes and retains durable identity and audit data.
Production provider and edge-log qualification remain deployment work.

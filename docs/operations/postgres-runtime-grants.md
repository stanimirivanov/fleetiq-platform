# PostgreSQL runtime grant contract

## TL;DR

Select the [versioned grant manifest](../../contracts/postgresql/runtime-grants.v1.json)
from the exact platform source revision recorded for the backend image. It defines
object privileges for distinct database identities. The database owner applies
and verifies the grants before enabling the corresponding processes.

A successful protected backend publication includes `runtime-grants.sql` in
its publication evidence. Select the artifact and image together using the
publication run and immutable digests. The SQL defines session-local functions.
Call them with the intended login roles from a maintenance transaction on the
drained primary. The consumer owns admission, operator monitoring access, and
credential retirement.

## Scope and authority

The contract covers checked serving, checked first-operator bootstrap, TCP
acceptance, durable replay, semantic interpretation, current projection,
analytics, notification delivery, and archive publication. The `commands` keys
are exact platform commands including required checked-schema flags. Combine
`common` with the one matching command. `schemas` grants USAGE, `select`,
`insert`, `update` and `delete` grant only the named table rights, `updateColumns`
grants UPDATE only on the listed columns, and `execute` identifies routines by
exact signature. PostgreSQL generates the declared identity values implicitly;
these runtime identities require no direct sequence rights for those inserts.
The contract adds no permissions for optional administrative Jobs.

Use a clean, dedicated FleetIQ database. The separate temporary migration
credential owns its schema and ledger. Apply `migrate` only with that identity
while all writers are fenced. Withdraw migration login and secret material after
checking the exact ledger and completing grants. Keep the ownership identity to
support future migrations. Give checked bootstrap its own restricted login and
withdraw it immediately after the bootstrap receipt is checked. Bootstrap uses
the normal checked command. Commission subsequent product state through the
authenticated product interface.

Runtime logins need different identities with no shared credentials. Provision
them without SUPERUSER, CREATEDB, CREATEROLE, REPLICATION or BYPASSRLS privilege,
without inherited roles or membership in the migration owner, and without
schema/object ownership or grant options. Review existing ACLs before rolling
out. Remove PUBLIC CONNECT and TEMPORARY on the dedicated database, CREATE on
`public`, and PUBLIC execution on application-owned routines in `analytics`,
`history`, `identity`, `ingestion`, `metadata` and `projection`. Explicitly grant
only the required named routines. Do not grant default table or sequence rights
to future objects. SQL authority is per command, while product code still owns
operator and tenant authorization.

PostgreSQL requires UPDATE privilege on at least one column for a row lock.
Where an existing immutability guard cannot supply that column, migration 0059
provides `runtime_lock_guard`, a constant virtual generated column. UPDATE on
that column permits `FOR SHARE` and `FOR UPDATE`. It does not allow the role to
change tenant lifecycle, replay checkpoints, or operator rule selection. The
migration adds columns and acquires DDL locks. Reserve a controlled migration
window before running checked processes with the new ledger.

The source transactions cross two invoker-trigger boundaries. A replay insert
into `ingestion.decoded_observations` must also enqueue `history.semantic_work`.
A semantic append to `history.asset_property_changes` must enqueue a projection
row and initialize its tenant head. The semantic writer also executes the
expression-index function `analytics.semantic_evidence_id`. Keep those rights
with the writer even when the receiving worker is stopped. Trigger failure
rolls back the parent write. Never grant an umbrella role to bypass it.

## Verify and publish

Prepare an owned disposable PostgreSQL 18 database named
`fleetiq_grant_contract_test`. Run the checked source revision’s normal `migrate`
command against that database with a temporary migration owner. Select a private
connection through psql's normal connection configuration. Run the fixture as
a disposable database administrator with `CREATEROLE` and the ability to apply
and test ACLs. Do not point it at an existing environment.

```bash
set -o pipefail
node scripts/verify-postgres-runtime-grants.mjs |
  psql -X -q -v ON_ERROR_STOP=1 -d fleetiq_grant_contract_test -f -
```

The fixture checks the complete successful migration ledger before ACL changes.
It uses one transaction, denies DDL and ledger writes for each restricted role,
exercises synthetic tenant locks and the replay, semantic and projection
handoffs, then checks archive bookkeeping, alert evidence and notification
checkpoint operations. It checks deferred constraints before rollback. Allow
ten seconds per statement and two seconds for locks. Retain the result with the
source revision, grant manifest SHA-256, PostgreSQL version and selected image
and migration inventory digests. The fixture grants no production credentials
and has no network or Kubernetes steps.

The fixture covers bounded database operations, so qualify complete command
runs, authenticated serving, credential withdrawal and capacity on the selected
installation before declaring installed acceptance. When a probe fails, capture
the exact denied object or missing ledger entry privately and compare it with the
reviewed source. A failed schema or privilege change requires a reviewed forward
fix or isolated recovery. Preserve successful ledger rows and retained data.

Publish the backend through the existing [image publication workflow](image-publication.md).
Select its source revision, image digest and migration inventory together with
the exact grant manifest digest. Deployment owns that later selection as well as
Secret provisioning, target identity checks, and authorized cluster execution.

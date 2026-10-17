# Bootstrap the first platform operator

## TL;DR

Run `fleetiq-server bootstrap-platform-operator` once from a trusted operator
environment with the four required environment variables. The command applies
migrations, creates the actor, exact external identity, and the initial
`platform.access.manage`, `identity.manage`, `tenant.provision`, and
`tenant.lifecycle.manage` grants atomically, then exits. Remove the bootstrap
environment values when it
finishes.

For production, first run explicit migration with separate DDL credentials,
then select `bootstrap-platform-operator --check-schema` with bootstrap-only DML
privileges. See [runtime and migrations](runtime-and-migrations.md). The unqualified
command retains development migrate-on-start compatibility.

## Preconditions

- PostgreSQL is reachable through `DATABASE_URL`.
- The selected authority ID names the trusted authentication configuration that
  will later validate this external subject.
- The external subject is copied exactly from that trusted authority. FleetIQ
  treats it as opaque and case-sensitive.
- Only an authorized installation operator can run the command or provide its
  environment.

The command establishes root platform authority and does not authenticate the
person invoking it. Do not expose it through a public service, startup probe, or
ordinary application route.

## Required environment

| Variable | Meaning |
|:--|:--|
| `DATABASE_URL` | PostgreSQL connection string for the installation. |
| `FLEETIQ_BOOTSTRAP_ACTOR_ID` | Stable internal FleetIQ actor identifier. |
| `FLEETIQ_BOOTSTRAP_AUTHORITY_ID` | Stable ID of the trusted authentication-authority configuration. |
| `FLEETIQ_BOOTSTRAP_EXTERNAL_SUBJECT` | Exact opaque subject asserted by that authority. |

The database URL can contain credentials, and the external subject can identify
a person. Keep both out of scripts, command arguments, logs, and source control.

## Run the command

For a locally built executable in PowerShell:

```powershell
$env:DATABASE_URL = '<installation database URL>'
$env:FLEETIQ_BOOTSTRAP_ACTOR_ID = '<stable internal actor ID>'
$env:FLEETIQ_BOOTSTRAP_AUTHORITY_ID = '<configured authority ID>'
$env:FLEETIQ_BOOTSTRAP_EXTERNAL_SUBJECT = '<exact external subject>'

cargo run --locked -p fleetiq-server -- bootstrap-platform-operator
```

A new installation prints:

```text
platform operator bootstrapped
```

Repeating the command with the same actor, authority, and subject prints:

```text
platform operator was already bootstrapped
```

The first successful operation owns its recorded time. An exact retry does not
recreate a grant that was deliberately revoked later.

## Failure and recovery

The actor, external identity, grant, and immutable bootstrap evidence commit in
one PostgreSQL transaction. If the command reports a connection, migration, or
bootstrap failure, correct the cause and rerun it with the same identity values.

A different actor, authority, or subject is rejected after bootstrap succeeds.
Changing the platform operator then requires a future authorized platform-grant
administration capability; bootstrap is not an authority-replacement mechanism.

## Clear the environment

After success or investigation, remove the command-specific values from the
PowerShell session:

```powershell
Remove-Item Env:FLEETIQ_BOOTSTRAP_ACTOR_ID
Remove-Item Env:FLEETIQ_BOOTSTRAP_AUTHORITY_ID
Remove-Item Env:FLEETIQ_BOOTSTRAP_EXTERNAL_SUBJECT
```

Manage `DATABASE_URL` according to the deployment's normal secret-injection
policy.

# Runtime, migration and OIDC trust contract

## TL;DR

Migrate explicitly with a temporary migration-owner role. Production serving and
bootstrap use `--check-schema` and roles without DDL. An exact immutable ledger
is required before work starts and while readiness is reported. Development
commands retain their original migrate-on-start behavior. This contract does
not provide a release orchestrator, online upgrades or automatic rollback.

## Commands

| Command | Database behavior | Other configuration |
|:--|:--|:--|
| `fleetiq-server migrate` | Apply and verify embedded migrations, close connections, exit | DATABASE_URL only; never opens HTTP |
| `fleetiq-server migration-inventory` | None | None; deterministic public JSON |
| `fleetiq-server serve --check-schema` | Validate ledger without DDL before listening | Existing bind and optional HTTP identity configuration |
| `fleetiq-server bootstrap-platform-operator --check-schema` | Validate ledger, perform existing atomic bootstrap, exit | Existing four bootstrap values |
| No arguments, `serve`, unqualified bootstrap | Apply and verify migrations for development compatibility | Existing settings |

No command accepts a connection string, identity subject or secret in argv.
Supply DATABASE_URL and existing credentials through the deployment's private
injection mechanism. Migration-only execution does not require working OIDC,
HTTP bind or bootstrap configuration. Help and inventory need no database.
Unknown commands, flags and extra arguments fail rather than guessing a mode.

## Database roles and first installation

1. The substrate/database owner provisions separate migration-owner and runtime
   roles. FleetIQ does not create PostgreSQL roles or accept a superuser fallback.
2. Stop/fence existing writers before a disruptive migration; for an empty first
   installation verify the target is actually empty independently.
3. Run the exact image's `migrate` command with temporary DDL credentials and
   bind its immutable image/source/migration inventory to release evidence.
4. Grant the runtime role CONNECT, USAGE on the product schemas and appropriate
   table/sequence DML permissions for the enabled application capabilities.
   Grant SELECT on `public._sqlx_migrations`. Do not grant CREATE, ownership,
   TRUNCATE, ledger mutation or migration-owner role membership.
5. Explicitly authorize first-operator bootstrap. Its temporary non-DDL role
   needs USAGE on identity, SELECT on the ledger, and SELECT/INSERT on
   `identity.platform_bootstrap`, `identity.actors`,
   `identity.external_actor_identities` and `identity.platform_action_grants`.
   Run checked bootstrap, then withdraw that credential.
6. Start checked serve with its runtime credential and complete identity config.
   Release tooling verifies health/product behavior before declaring acceptance.

Privileges are provisioning policy, not SQL embedded in this binary. Default
privileges for future tables must be reviewed with their new runtime capability.
Separate processes with the same password or schema ownership are not separate
authorities. Revoke CREATE on public for ordinary roles where necessary; never
make the application role an owner. The local development superuser is not a
production privilege model.

## Exact compatibility and errors

The binary accepts only its complete embedded version/SHA-384 set, with every
ledger row successful. Empty/missing history, gaps, changed checksums, failed
migrations, unknown newer versions, denied access and connectivity failures
reject startup. A single SELECT reads at most the known count plus one. This
bounds probes and avoids a multi-query snapshot race; it neither locks out DDL
nor detects privileged manual changes to product tables.

Liveness remains `/health/live` (204 while HTTP runs). Readiness
`/health/ready` returns 204 for compatible accessible history, otherwise 503
with no database details. Fix the actual schema/privilege or connection cause;
do not edit checksums, remove unknown history or let runtime migrate as recovery.
Normal process errors omit database/SQL/provider diagnostics; trusted Rust error
causes remain available internally and must not be recursively dumped into logs.

Migration sessions explicitly select the public search path so role-specific
schemas cannot redirect migration ledger lookup. Their connection
is closed instead of returning its settings/locks to runtime traffic. Check mode
changes no session settings and executes no DDL. Checksum projections read at
most 49 bytes, distinguishing oversized values from valid 48-byte SHA-384 digests.

SQLx holds its database advisory lock during migration. Two migration commands
serialize; repeated success is idempotent. Failed transactions are not recorded
as applied. The command closes its pool after failure, releasing session locks.
A connection/commit failure may leave uncertain outcome: inspect actual history
and release evidence, then retry the same immutable artifact only after review.
This lock does not stop HTTP writers or replace an environment release lock.

## Inventory and producer handoff

`migration-inventory` emits one JSON object followed by a newline:
`format` is `fleetiq-migrations.v1`, `checksum_algorithm` is `sha384`,
and `migrations` is an ascending array of decimal-text version, description and
96-character lower-case checksum. Decimal text preserves the entire signed-64-bit
version range in consumers that parse JSON numbers as floating point. Checksums come from SQLx's embedded inventory,
not a second hashing implementation. Output contains no SQL or environment.

D02 must bind this public document to the published backend image and source
revision; D04 must select checked serve/bootstrap and separate job credentials.
These packages and Kubernetes activation are not implemented by D01.

## Mounted OIDC CA trust

Optionally set `FLEETIQ_OIDC_ADDITIONAL_CA_BUNDLE` to a read-only mounted public
PEM certificate file. All five existing browser settings remain required.
Setting trust without browser mode is an error; an empty path is invalid.

The bundle adds up to 32 roots (maximum 256 KiB) to normal trust. It accepts LF
and CRLF and certificate blocks separated by whitespace. Private-key blocks,
trailing junk, missing files, empty files and invalid certificates fail startup.
The same client performs discovery, JWKS refresh and token exchange; HTTPS,
hostname/issuer checks, no redirects, ten-second timeouts and the existing
one-MiB response limit remain. No global trust-store installation or insecure
flag is supported. Only public certificates belong in the mounted bundle.

Mount trust before startup and restart after rotation. Existing public-origin,
callback, secure-cookie and CSRF rules are unchanged. Tests prove actual TLS
trust/rejection on loopback; a customer issuer and ingress remain separate D10
qualification. See [browser identity](browser-identity.md).

## Shutdown and recovery

Ctrl+C and Unix SIGTERM initiate graceful stop: stop accepting HTTP, drain
accepted requests, cancel/join expired browser-record cleanup, close connections.
The supervisor owns its termination grace deadline; forced kill or host loss
requires release/database recovery. Do not infer that a Pod deletion proves all
writers stopped: observe actual processes, requests and database sessions.
Current writers include HTTP metadata/identity commands, login/logout/session
writes and browser-record cleanup. Future ingestion/processing/replay workers
must be added to the release inventory before deployment.

Migration/rollout failures require maintenance, diagnosis and a reviewed forward
fix or isolated restore. No automatic down migration, incompatible binary restart
or data/PVC deletion is performed. Existing development Compose remains a
separate diagnostic until the Kubernetes plan proves operating/recovery parity.

## Container readiness command

The shell-free production image uses `fleetiq-server healthcheck` instead of
curl. It requests only loopback port 8080's `/health/ready`, requires HTTP 204,
refuses redirects/proxies and times out after two seconds. It does not read
startup credentials or perform migrations. See [image production](image-publication.md)
for runtime packaging and qualification.

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readdirSync, readFileSync } from 'node:fs';
import { grantStatements } from './images/runtime-grants.mjs';

const directory = new URL('../contracts/postgresql/', import.meta.url);
const migrationsDirectory = new URL('../migrations/', import.meta.url);
const contract = JSON.parse(readFileSync(new URL('runtime-grants.v1.json', directory)));
const sha384 = (bytes) => createHash('sha384').update(bytes).digest('hex');
const migrations = readdirSync(migrationsDirectory)
  .filter((name) => /^\d{4,}_[a-z0-9_]+\.sql$/u.test(name))
  .sort((a, b) => Number(a.split('_')[0]) - Number(b.split('_')[0]))
  .map((name) => ({
    version: Number(name.split('_')[0]),
    sha384: sha384(readFileSync(new URL(name, migrationsDirectory))),
  }));
assert.equal(contract.format, 'fleetiq-postgresql-runtime-grants.v1');
assert.equal(contract.postgresMajor, 18);
assert.deepEqual(contract.migrationHead, migrations.at(-1));
assert.deepEqual(contract.common.database, ['CONNECT']);
assert.deepEqual(contract.common.select, ['public._sqlx_migrations']);

const write = (line) => process.stdout.write(`${line}\n`);
// The fixture uses one transaction, including role creation and ACL changes.
// Refuse a wrong database or ledger before touching any privileges.
write(`BEGIN;
SET LOCAL statement_timeout = '10s';
SET LOCAL lock_timeout = '2s';
DO $$ BEGIN
  IF current_database() <> 'fleetiq_grant_contract_test'
      OR current_setting('server_version_num')::integer / 10000 <> 18 THEN
    RAISE EXCEPTION 'Expected a disposable PostgreSQL 18 grant contract database';
  END IF;
  IF (SELECT count(*) FROM public._sqlx_migrations) <> ${migrations.length}
      OR EXISTS (
        SELECT 1 FROM (VALUES
          ${migrations.map(({ version, sha384 }) => `(${version}, '${sha384}')`).join(',\n          ')}
        ) expected(version, checksum)
        LEFT JOIN public._sqlx_migrations actual USING (version)
        WHERE actual.success IS DISTINCT FROM true
           OR encode(actual.checksum, 'hex') IS DISTINCT FROM expected.checksum
      ) THEN
    RAISE EXCEPTION 'Grant contract requires the exact successful migration ledger';
  END IF;
END $$;
REVOKE CONNECT, TEMPORARY ON DATABASE fleetiq_grant_contract_test FROM PUBLIC;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
REVOKE EXECUTE ON ALL FUNCTIONS IN SCHEMA analytics, history, identity, ingestion, metadata, projection FROM PUBLIC;`);
for (const [command, grants] of Object.entries(contract.commands)) {
  const role = `grant_test_${command.split(' ')[0].replaceAll('-', '_')}`;
  assert.match(role, /^grant_test_[a-z_]+$/u);
  write(`CREATE ROLE ${role} NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;`);
  for (const statement of grantStatements('fleetiq_grant_contract_test', role, grants))
    write(statement);
  write(`SET LOCAL ROLE ${role};
DO $$ BEGIN
  IF (SELECT count(*) FROM public._sqlx_migrations WHERE success) <> ${migrations.length} THEN
    RAISE EXCEPTION 'Runtime identity cannot read the complete ledger';
  END IF;
  IF pg_has_role(current_user, session_user, 'MEMBER')
      OR has_database_privilege(current_user, current_database(), 'TEMP') THEN
    RAISE EXCEPTION 'Runtime identity inherited owner access or a temporary schema';
  END IF;
  BEGIN UPDATE public._sqlx_migrations SET success = false;
    RAISE EXCEPTION 'Runtime identity can mutate migration history';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN CREATE TABLE public.grant_test_unexpected (id integer);
    RAISE EXCEPTION 'Runtime identity can create a table';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN ALTER TABLE metadata.tenants ADD COLUMN grant_test_unexpected integer;
    RAISE EXCEPTION 'Runtime identity can alter product schema';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
RESET ROLE;`);
}
write(readFileSync(new URL('runtime-grants-check.sql', directory), 'utf8'));
const retention = new URL('../crates/postgres/src/history/retention/', import.meta.url);
const candidates = readFileSync(new URL('candidates.sql', retention), 'utf8');
const context = readFileSync(new URL('context.sql', retention), 'utf8');
write(`SET LOCAL ROLE grant_test_serve;
DO $$ DECLARE candidate jsonb; context jsonb;
BEGIN
    EXECUTE $grant_query$SELECT to_jsonb(result) FROM (${context}) result$grant_query$
        INTO STRICT context USING 'grant-contract';
    EXECUTE $grant_query$SELECT to_jsonb(result) FROM (${candidates}) result$grant_query$
        INTO STRICT candidate USING 'grant-contract';
    IF context->>'revision!' IS DISTINCT FROM '1'
       OR candidate->>'id!' IS NULL THEN
        RAISE EXCEPTION 'Restricted serving failed to collect retention preview';
    END IF;
END $$;
RESET ROLE;`);
write(`ROLLBACK;\n\\echo 'PostgreSQL runtime grant fixture passed'`);

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readdirSync, readFileSync } from 'node:fs';

const directory = new URL('../contracts/postgresql/', import.meta.url);
const migrationsDirectory = new URL('../migrations/', import.meta.url);
const contract = JSON.parse(readFileSync(new URL('runtime-grants.v1.json', directory)));
const identifier = '[a-z_][a-z_0-9]*';
const qualified = `${identifier}\\.${identifier}`;
const relation = new RegExp(`^${qualified}$`, 'u');
const signature = new RegExp(`^${qualified}\\((?:[a-z]+(?:,[a-z]+)*)?\\)$`, 'u');
const check = (name, pattern) => {
  assert.match(name, pattern);
  return name;
};
const names = (items, pattern) => items.map((item) => check(item, pattern)).join(', ');
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
  const role = check(`grant_test_${command.split(' ')[0].replaceAll('-', '_')}`, /^grant_test_[a-z_]+$/u);
  const schemaNames = names(grants.schemas, new RegExp(`^${identifier}$`, 'u'));
  write(`CREATE ROLE ${role} NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;`);
  write(`GRANT CONNECT ON DATABASE fleetiq_grant_contract_test TO ${role};`);
  write(`GRANT USAGE ON SCHEMA ${schemaNames} TO ${role};`);
  write(`GRANT SELECT ON TABLE public._sqlx_migrations TO ${role};`);
  for (const action of ['select', 'insert', 'update', 'delete']) {
    if (grants[action]?.length)
      write(`GRANT ${action.toUpperCase()} ON TABLE ${names(grants[action], relation)} TO ${role};`);
  }
  for (const [table, columns] of Object.entries(grants.updateColumns ?? {}))
    write(`GRANT UPDATE (${names(columns, new RegExp(`^${identifier}$`, 'u'))}) ON TABLE ${check(table, relation)} TO ${role};`);
  if (grants.execute?.length)
    write(`GRANT EXECUTE ON FUNCTION ${names(grants.execute, signature)} TO ${role};`);
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
write(`ROLLBACK;\n\\echo 'PostgreSQL runtime grant fixture passed'`);

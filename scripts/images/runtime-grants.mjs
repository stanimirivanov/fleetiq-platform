import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../..", import.meta.url));
const databaseName = "fleetiq";
const productSchemas = "analytics, history, identity, ingestion, metadata, projection";
const databaseRole = (command) => `fleetiq_${command.split(" ")[0].replaceAll("-", "_")}`;
const identifier = '[a-z_][a-z_0-9]*';
const relation = `${identifier}\\.${identifier}`;
const identifierPattern = new RegExp(`^${identifier}$`, 'u');
const relationPattern = new RegExp(`^${relation}$`, 'u');
const routinePattern = new RegExp(`^${relation}\\((?:[a-z]+(?:,[a-z]+)*)?\\)$`, 'u');

function names(items, pattern) {
  return items
    .map((item) => {
      assert.match(item, pattern);
      return item;
    })
    .join(', ');
}

function ledgerCheck(migrations) {
  const expected = migrations
    .map(({ version, checksum }) => `(${version}, '${checksum}')`)
    .join(',\n      ');
  return `
CREATE FUNCTION pg_temp.fleetiq_check_ledger(prefix boolean) RETURNS void
LANGUAGE plpgsql SET search_path = pg_catalog, public AS $$ BEGIN
  IF current_database() <> '${databaseName}'
      OR current_setting('server_version_num')::integer / 10000 <> 18
      OR pg_is_in_recovery() THEN
    RAISE EXCEPTION 'Expected the PostgreSQL 18 FleetIQ primary';
  END IF;
  IF to_regclass('public._sqlx_migrations') IS NULL THEN
    IF prefix THEN RETURN; END IF;
    RAISE EXCEPTION 'FleetIQ migration ledger missing';
  END IF;
  IF EXISTS (
    SELECT 1 FROM public._sqlx_migrations a
    LEFT JOIN (VALUES ${expected}) e(version, checksum) USING (version)
    WHERE a.success IS DISTINCT FROM true OR encode(a.checksum, 'hex') IS DISTINCT FROM e.checksum
  ) OR EXISTS (
    SELECT 1 FROM (VALUES ${expected}) e(version, checksum)
    WHERE (NOT prefix OR e.version <= (SELECT max(version) FROM public._sqlx_migrations)) AND
          NOT EXISTS (SELECT FROM public._sqlx_migrations a WHERE a.version = e.version)
  ) THEN
    RAISE EXCEPTION 'FleetIQ migration ledger differs';
  END IF;
END $$;`;
}

export function grantStatements(database, role, grants) {
  assert.match(database, identifierPattern);
  assert.match(role, identifierPattern);
  const acl = [
    `GRANT CONNECT ON DATABASE ${database} TO ${role};`,
    `GRANT USAGE ON SCHEMA ${names(grants.schemas, identifierPattern)} TO ${role};`,
    `GRANT SELECT ON TABLE public._sqlx_migrations TO ${role};`,
  ];
  for (const action of ['select', 'insert', 'update', 'delete']) {
    if (grants[action]?.length)
      acl.push(
        `GRANT ${action.toUpperCase()} ON TABLE ${names(grants[action], relationPattern)} TO ${role};`,
      );
  }
  for (const [table, columns] of Object.entries(grants.updateColumns ?? {}))
    acl.push(
      `GRANT UPDATE (${names(columns, identifierPattern)}) ON TABLE ${names([table], relationPattern)} TO ${role};`,
    );
  if (grants.execute?.length)
    acl.push(
      `GRANT EXECUTE ON FUNCTION ${names(grants.execute, routinePattern)} TO ${role};`,
    );
  return acl;
}

function expectedPrivileges(contract) {
  const values = [];
  for (const [command, grants] of Object.entries(contract.commands)) {
    const role = databaseRole(command);
    const add = (type, object, privilege) =>
      values.push(`('${role}', '${type}', '${object}', '${privilege}', false)`);
    add('database', databaseName, 'CONNECT');
    add('relation', 'public._sqlx_migrations', 'SELECT');
    for (const schema of grants.schemas) add('schema', schema, 'USAGE');
    for (const privilege of ['select', 'insert', 'update', 'delete'])
      for (const name of grants[privilege] ?? [])
        add('relation', name, privilege.toUpperCase());
    for (const [table, columns] of Object.entries(grants.updateColumns ?? {}))
      for (const column of columns) add('column', `${table}.${column}`, 'UPDATE');
    for (const routine of grants.execute ?? []) add('function', routine, 'EXECUTE');
  }
  values.push("('PUBLIC', 'schema', 'public', 'USAGE', false)");
  return values.join(',\n');
}

function verifyPrivileges(contract) {
  const schemas =
    "'analytics', 'history', 'identity', 'ingestion', 'metadata', 'projection', 'public'";
  return `
CREATE FUNCTION pg_temp.fleetiq_verify_runtime_grants(selected name[]) RETURNS void
LANGUAGE plpgsql SET search_path = pg_catalog, public AS $$ BEGIN
  IF EXISTS (
    SELECT FROM pg_roles r
    WHERE r.rolname IN (${Object.keys(contract.commands).map((command) => `'${databaseRole(command)}'`).join(', ')})
      AND r.rolname <> ALL(selected)
      AND (r.rolcanlogin OR has_database_privilege(r.oid, current_database(), 'CONNECT')
        OR EXISTS (SELECT FROM pg_auth_members m WHERE m.roleid = r.oid))
  ) THEN RAISE EXCEPTION 'An unselected database identity still has authority'; END IF;
  IF EXISTS (
    SELECT FROM pg_namespace n CROSS JOIN pg_roles r
    WHERE n.nspname NOT IN (${schemas}, 'information_schema')
      AND left(n.nspname, 3) <> 'pg_'
      AND r.rolname = ANY(selected)
      AND (has_schema_privilege(r.oid, n.oid, 'USAGE')
        OR has_schema_privilege(r.oid, n.oid, 'CREATE'))
  ) THEN RAISE EXCEPTION 'Unintended access to an additional user schema'; END IF;
  IF EXISTS (
    WITH acl(kind, object, grantee, privilege, grantable) AS (
      SELECT 'database', d.datname::text, x.grantee, x.privilege_type, x.is_grantable
        FROM pg_database d, LATERAL aclexplode(coalesce(d.datacl, acldefault('d', d.datdba))) x
       WHERE d.datname = current_database()
      UNION ALL
      SELECT 'schema', n.nspname, x.grantee, x.privilege_type, x.is_grantable
        FROM pg_namespace n, LATERAL aclexplode(coalesce(n.nspacl, acldefault('n', n.nspowner))) x
       WHERE n.nspname IN (${schemas})
      UNION ALL
      SELECT 'relation', n.nspname || '.' || c.relname, x.grantee, x.privilege_type, x.is_grantable
        FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace,
             LATERAL aclexplode(c.relacl) x WHERE n.nspname IN (${schemas})
      UNION ALL
      SELECT 'column', n.nspname || '.' || c.relname || '.' || a.attname, x.grantee, x.privilege_type, x.is_grantable
        FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace,
             LATERAL aclexplode(a.attacl) x WHERE n.nspname IN (${schemas})
      UNION ALL
      SELECT 'function', n.nspname || '.' || p.proname || '(' || replace(oidvectortypes(p.proargtypes), ', ', ',') || ')',
             x.grantee, x.privilege_type, x.is_grantable
        FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace,
             LATERAL aclexplode(coalesce(p.proacl, acldefault('f', p.proowner))) x
       WHERE n.nspname IN (${schemas})
    ), observed AS (
      SELECT coalesce(r.rolname::text, 'PUBLIC'), a.kind, a.object, a.privilege, a.grantable
        FROM acl a LEFT JOIN pg_roles r ON a.grantee = r.oid
       WHERE a.grantee = 0 OR r.rolname = ANY(selected)
    ), all_expected(role_name, kind, object, privilege, grantable) AS (VALUES ${expectedPrivileges(contract)}),
    expected AS (SELECT * FROM all_expected WHERE role_name = 'PUBLIC' OR role_name::name = ANY(selected))
    (SELECT * FROM expected EXCEPT SELECT * FROM observed)
      UNION ALL
    (SELECT * FROM observed EXCEPT SELECT * FROM expected)
  ) THEN RAISE EXCEPTION 'FleetIQ object grants differ from the producer contract'; END IF;
END $$;`;
}

/** Build the SQL artifact from this revision and the qualified image inventory. */
export function runtimeGrantSql(inventory, sourceRoot = root) {
  const contract = JSON.parse(readFileSync(
    join(sourceRoot, "contracts/postgresql/runtime-grants.v1.json"),
  ));
  assert.equal(contract.format, "fleetiq-postgresql-runtime-grants.v1");
  assert.equal(contract.postgresMajor, 18);
  const head = inventory.migrations.at(-1);
  assert.equal(head.version, String(contract.migrationHead.version));
  assert.equal(head.checksum, contract.migrationHead.sha384);
  for (const { version, checksum } of inventory.migrations) {
    assert.match(version, /^[1-9][0-9]*$/u);
    assert.match(checksum, /^[a-f0-9]{96}$/u);
  }
  const grants = Object.entries(contract.commands).map(([command, policy]) =>
    `  IF '${databaseRole(command)}' = ANY(selected) THEN
    ${grantStatements(databaseName, databaseRole(command), policy).join("\n    ")}
  END IF;`,
  ).join("\n");
  return `-- FleetIQ PostgreSQL runtime grants and ledger from the qualified backend.
${ledgerCheck(inventory.migrations)}
CREATE FUNCTION pg_temp.fleetiq_apply_runtime_grants(selected name[]) RETURNS void
LANGUAGE plpgsql SET search_path = pg_catalog, public AS $$ BEGIN
  REVOKE CREATE ON SCHEMA public FROM PUBLIC;
  REVOKE EXECUTE ON ALL ROUTINES IN SCHEMA ${productSchemas} FROM PUBLIC;
  ALTER DEFAULT PRIVILEGES REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
${grants}
END $$;
${verifyPrivileges(contract)}
`;
}

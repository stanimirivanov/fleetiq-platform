/** Exercise the loaded runtime whose config digest was checked against the OCI layout. */
import { join } from "node:path";
import { docker, jsonDocker } from "./process.mjs";
import { until } from "./resources.mjs";
import { ensure, CHECKS, readJson } from "./policy.mjs";
import { validateInventory } from "./inventory.mjs";
export async function qualifyRuntime(
  image,
  tools,
  artifact,
  root,
  resources,
  directory,
) {
  const [loaded] = await jsonDocker(["image", "inspect", image]);
  ensure(
    loaded.Id === artifact.config,
    "Loaded runtime config differs from exported OCI artifact.",
  );
  const pg = await resources.container("postgres", [
    "--tmpfs",
    "/var/lib/postgresql:rw,nosuid,size=256m",
    "-e",
    "POSTGRES_USER=fleetiq",
    "-e",
    "POSTGRES_PASSWORD=fleetiq-image-only",
    "-e",
    "POSTGRES_DB=fleetiq",
    tools.postgres,
  ]);
  await until(async () => {
    try {
      await docker([
        "exec",
        pg,
        "pg_isready",
        "-h",
        "127.0.0.1",
        "-U",
        "fleetiq",
        "-d",
        "fleetiq",
      ]);
      return true;
    } catch {
      return false;
    }
  });
  const ownerUrl =
    "postgres://fleetiq:fleetiq-image-only@" + pg + ":5432/fleetiq";
  const restrictions = [
    "--read-only",
    "--cap-drop",
    "ALL",
    "--security-opt",
    "no-new-privileges:true",
    "--tmpfs",
    "/tmp:rw,noexec,nosuid,size=16m",
  ];
  const empty = await resources.container("empty", [
    ...restrictions,
    "-e",
    "DATABASE_URL=" + ownerUrl,
    image,
  ]);
  ensure(
    (await docker(["wait", empty], { timeout: 20_000 })).trim() === "1",
    "Checked image accepted an unmigrated database.",
  );
  const migrated = await docker(
    [
      "run",
      "--rm",
      "--network",
      resources.network,
      ...restrictions,
      "-e",
      "DATABASE_URL=" + ownerUrl,
      "-e",
      "FLEETIQ_BIND_ADDRESS=invalid",
      image,
      "migrate",
    ],
    { timeout: 120_000 },
  );
  ensure(
    migrated.trim() === "embedded migrations applied and verified",
    "Migration-only image command failed.",
  );
  await docker([
    "exec",
    pg,
    "psql",
    "-U",
    "fleetiq",
    "-d",
    "fleetiq",
    "-v",
    "ON_ERROR_STOP=1",
    "-c",
    "CREATE ROLE runtime LOGIN PASSWORD 'runtime-image-only' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT; REVOKE CREATE ON SCHEMA public FROM PUBLIC; REVOKE CREATE,TEMPORARY ON DATABASE fleetiq FROM PUBLIC; GRANT USAGE ON SCHEMA public TO runtime; GRANT SELECT ON public._sqlx_migrations TO runtime;",
  ]);
  const ddl = await docker([
    "exec",
    pg,
    "psql",
    "-U",
    "runtime",
    "-d",
    "fleetiq",
    "-Atc",
    "SELECT current_user='runtime' AND NOT has_database_privilege(current_user,current_database(),'CREATE') AND NOT has_database_privilege(current_user,current_database(),'TEMPORARY') AND NOT EXISTS (SELECT 1 FROM pg_namespace WHERE has_schema_privilege(current_user,oid,'CREATE')) AND NOT rolsuper FROM pg_roles WHERE rolname=current_user",
  ]);
  ensure(ddl.trim() === "t", "Runtime fixture does not prove denied DDL.");
  const runtimeUrl =
    "postgres://runtime:runtime-image-only@" + pg + ":5432/fleetiq";
  const server = await resources.container("server", [
    ...restrictions,
    "--publish",
    "127.0.0.1::8080",
    "-e",
    "DATABASE_URL=" + runtimeUrl,
    image,
  ]);
  const [metadata] = await jsonDocker(["inspect", server]);
  ensure(
    metadata.HostConfig.ReadonlyRootfs &&
      JSON.stringify(metadata.HostConfig.CapDrop) === '["ALL"]' &&
      metadata.HostConfig.SecurityOpt.includes("no-new-privileges:true"),
    "Runtime isolation differs.",
  );
  const port = metadata.NetworkSettings.Ports["8080/tcp"][0].HostPort;
  const origin = "http://127.0.0.1:" + port;
  await until(async () => {
    try {
      return (
        (
          await fetch(origin + "/health/ready", {
            signal: AbortSignal.timeout(1000),
          })
        ).status === 204
      );
    } catch {
      return false;
    }
  });
  ensure(
    (
      await fetch(origin + "/health/live", {
        signal: AbortSignal.timeout(3000),
      })
    ).status === 204,
    "Liveness failed.",
  );
  ensure(
    (await fetch(origin + "/api/v1", { signal: AbortSignal.timeout(3000) }))
      .status === 200,
    "Discovery failed.",
  );
  ensure(
    metadata.Config.User === "10001:10001",
    "Runtime process is not nonroot.",
  );
  await docker([
    "exec",
    server,
    "/usr/local/bin/fleetiq-server",
    "healthcheck",
  ]);
  await docker(["stop", "--time", "10", server], { timeout: 20_000 });
  const [stopped] = await jsonDocker(["inspect", server]);
  ensure(stopped.State.ExitCode === 0, "SIGTERM was not graceful.");
  const inventory = JSON.parse(
    await docker([
      "run",
      "--rm",
      "--network",
      "none",
      ...restrictions,
      image,
      "migration-inventory",
    ]),
  );
  validateInventory(inventory, root);
  // Docker's archive API reads the ordinary image filesystem; no shell/cat is
  // installed in the minimal runtime. The copied inventory is bounded on read.
  const embeddedPath = join(directory, "embedded-migrations.json");
  await docker([
    "cp",
    server + ":/usr/share/fleetiq/migrations.json",
    embeddedPath,
  ]);
  const embedded = readJson(embeddedPath);
  ensure(
    JSON.stringify(embedded) === JSON.stringify(inventory),
    "Embedded inventory differs from the binary.",
  );
  await docker([
    "exec",
    pg,
    "psql",
    "-U",
    "fleetiq",
    "-d",
    "fleetiq",
    "-v",
    "ON_ERROR_STOP=1",
    "-c",
    "UPDATE public._sqlx_migrations SET checksum=decode('00','hex') WHERE version=(SELECT min(version) FROM public._sqlx_migrations)",
  ]);
  const stale = await resources.container("stale", [
    ...restrictions,
    "-e",
    "DATABASE_URL=" + runtimeUrl,
    image,
  ]);
  ensure(
    (await docker(["wait", stale], { timeout: 20_000 })).trim() === "1",
    "Checked image accepted modified migration history.",
  );
  return { inventory, checks: CHECKS.slice(0, -3) };
}

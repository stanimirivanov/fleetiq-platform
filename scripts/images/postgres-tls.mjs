import { chmodSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { docker, run } from "./process.mjs";
import { until } from "./resources.mjs";

export const wrongDatabaseHostname = "unverified-database-name";

export async function configurePostgresTls(directory, pg) {
  const tls = join(directory, "postgres-tls");
  mkdirSync(tls, { mode: 0o700 });
  const file = (name) => join(tls, name);
  const openssl = (args) => run("openssl", args, { timeout: 15_000 });
  try {
    for (const name of ["ca", "unrelated-ca"]) {
      await openssl([
        "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2",
        "-subj", "/CN=FleetIQ disposable " + name,
        "-keyout", file(name + ".key"), "-out", file(name + ".crt"),
      ]);
    }
    await openssl([
      "req", "-new", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=" + pg,
      "-keyout", file("server.key"), "-out", file("server.csr"),
    ]);
    writeFileSync(file("extensions"), "subjectAltName=DNS:" + pg + "\n", {
      mode: 0o600,
    });
    await openssl([
      "x509", "-req", "-in", file("server.csr"), "-CA", file("ca.crt"),
      "-CAkey", file("ca.key"), "-CAcreateserial", "-days", "2",
      "-extfile", file("extensions"), "-out", file("server.crt"),
    ]);
    for (const name of ["server.key", "server.crt"]) {
      const target = "/tmp/fleetiq-" + name;
      await docker(["cp", file(name), pg + ":" + target]);
      await docker(["exec", "--user", "0", pg, "chown", "postgres:postgres", target]);
      await docker(["exec", "--user", "0", pg, "chmod", "0600", target]);
    }
  } finally {
    for (const name of ["ca.key", "unrelated-ca.key", "server.key"])
      rmSync(file(name), { force: true });
  }
  await docker([
    "exec", pg, "psql", "-X", "-U", "fleetiq", "-d", "fleetiq", "-v", "ON_ERROR_STOP=1",
    "-c", "ALTER SYSTEM SET ssl_cert_file='/tmp/fleetiq-server.crt'",
    "-c", "ALTER SYSTEM SET ssl_key_file='/tmp/fleetiq-server.key'",
    "-c", "ALTER SYSTEM SET ssl=on", "-c", "SELECT pg_reload_conf()",
  ]);
  await until(async () =>
    (await docker([
      "exec", pg, "psql", "-X", "-U", "fleetiq", "-d", "fleetiq", "-Atc", "SHOW ssl",
    ])).trim() === "on",
  );
  for (const name of ["ca.crt", "unrelated-ca.crt"]) chmodSync(file(name), 0o644);
  return ["--mount", "type=bind,src=" + file("ca.crt") + ",dst=/etc/fleetiq-tls/ca.crt,readonly",
    "--mount", "type=bind,src=" + file("unrelated-ca.crt") + ",dst=/etc/fleetiq-tls/unrelated-ca.crt,readonly"];
}

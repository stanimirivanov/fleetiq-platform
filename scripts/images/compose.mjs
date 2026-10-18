/** Development Compose consumes the already-built production bytes, with explicit dev mode. */
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { docker, jsonDocker } from "./process.mjs";
import { ensure } from "./policy.mjs";
export async function qualifyCompose(image, directory, resources, tools) {
  const server = resources.network + "-dev-server";
  const postgres = resources.network + "-dev-postgres";
  resources.containers.push(server, postgres);
  const label = resources.label + ": " + resources.id;
  const override = `services:
  server:
    image: ${image}
    container_name: ${server}
    labels:
      ${label}
    ports: !override ["127.0.0.1::8080"]
  postgres:
    image: ${tools.postgres}
    container_name: ${postgres}
    labels:
      ${label}
    ports: !reset []
    volumes: !override []
    tmpfs: ["/var/lib/postgresql:rw,nosuid,size=256m"]
volumes: !reset {}
networks:
  default:
    external: true
    name: ${resources.network}
`;
  writeFileSync(join(directory, "compose.yaml"), override);
  writeFileSync(join(directory, "empty.env"), "");
  const env = {
    ...process.env,
    FLEETIQ_DATABASE_URL:
      "postgres://fleetiq:fleetiq-local-only@postgres:5432/fleetiq",
  };
  await docker(
    [
      "compose",
      "--project-name",
      resources.network + "-dev",
      "--env-file",
      join(directory, "empty.env"),
      "-f",
      "deploy/local/compose.yaml",
      "-f",
      join(directory, "compose.yaml"),
      "up",
      "--no-build",
      "--wait",
      "--wait-timeout",
      "90",
    ],
    { capture: false, timeout: 120_000, env },
  );
  const [metadata] = await jsonDocker(["inspect", server]);
  ensure(
    metadata.Config.User === "10001:10001" &&
      metadata.HostConfig.ReadonlyRootfs &&
      JSON.stringify(metadata.HostConfig.CapDrop) === '["ALL"]' &&
      metadata.HostConfig.SecurityOpt.includes("no-new-privileges:true"),
    "Development Compose isolation differs.",
  );
  const origin =
    "http://127.0.0.1:" +
    metadata.NetworkSettings.Ports["8080/tcp"][0].HostPort;
  for (const path of ["/health/live", "/health/ready"])
    ensure(
      (await fetch(origin + path, { signal: AbortSignal.timeout(3000) }))
        .status === 204,
      "Development Compose health failed.",
    );
  await docker(["stop", "--time", "10", server]);
  ensure(
    (await jsonDocker(["inspect", server]))[0].State.ExitCode === 0,
    "Development Compose shutdown was not graceful.",
  );
  ensure(
    !(
      await docker([
        "volume",
        "ls",
        "--filter",
        "label=com.docker.compose.project=" + resources.network + "-dev",
        "--format",
        "{{.Name}}",
      ])
    ).trim(),
    "Disposable Compose fixture unexpectedly created persistent storage.",
  );
}

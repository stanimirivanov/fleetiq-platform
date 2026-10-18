/** Skopeo preserves the full image index and attestations; no custom registry client. */
import { rmSync } from "node:fs";
import { join } from "node:path";
import { docker, jsonDocker, containerUser } from "./process.mjs";
import { until } from "./resources.mjs";
import { ensure, DIGEST, hash } from "./policy.mjs";
function sandbox(directory, readOnly) {
  return [
    "--user",
    containerUser(),
    "--read-only",
    "--cap-drop",
    "ALL",
    "--security-opt",
    "no-new-privileges:true",
    "--env",
    "TMPDIR=/tmp",
    "--env",
    "HOME=/tmp",
    "--tmpfs",
    "/tmp:rw,noexec,nosuid,size=512m",
    "--volume",
    directory + ":/artifact" + (readOnly ? ":ro" : ""),
  ];
}
export function transport(
  tools,
  directory,
  args,
  extra = [],
  readOnly = false,
) {
  return docker(
    [
      "run",
      "--rm",
      ...sandbox(directory, readOnly),
      ...extra,
      tools.transport,
      "--tmpdir",
      "/tmp",
      ...args,
    ],
    { timeout: 300_000 },
  );
}
export function validateLoadedIdentity(metadata, expected) {
  ensure(
    DIGEST.test(expected) && metadata?.Id === expected,
    "Loaded image ownership changed.",
  );
}
export function validateLoadedConfig(raw, expected) {
  ensure(
    DIGEST.test(expected) && "sha256:" + hash(raw) === expected,
    "Loaded runtime config differs from exported OCI artifact.",
  );
}
export async function loadRuntime(
  tools,
  directory,
  tag,
  resources,
  expectedConfig,
  operations = { docker, jsonDocker, transport },
) {
  const {
    docker: command,
    jsonDocker: inspect,
    transport: transfer,
  } = operations;
  // Keep large temporary writes on the daemon's filesystem. Native binary output
  // avoids slow bind-mount writes and does not expose the daemon socket.
  const archive = await resources.container(
    "archive",
    [
      ...sandbox(directory, true),
      "--entrypoint",
      "/usr/bin/sleep",
      tools.transport,
      "infinity",
    ],
    "none",
  );
  console.log("Converting OCI runtime into bounded isolated scratch.");
  await command(
    [
      "exec",
      archive,
      "skopeo",
      "--tmpdir",
      "/tmp",
      "--override-os",
      "linux",
      "--override-arch",
      "amd64",
      "copy",
      "oci:/artifact/layout",
      "docker-archive:/tmp/runtime.tar:" + tag,
    ],
    { timeout: 900_000 },
  );
  console.log("Streaming runtime archive to a native binary file.");
  const archivePath = join(directory, "import.tar");
  // Do not use the daemon archive API: process-level tmpfs is only visible
  // inside the container. Docker exec streams raw bytes to a native host file.
  await command(["exec", archive, "cat", "/tmp/runtime.tar"], {
    output: archivePath,
    timeout: 300_000,
  });
  console.log("Loading the exact runtime archive into Docker.");
  await command(["load", "--input", archivePath], {
    timeout: 600_000,
  });
  const [loaded] = await inspect(["image", "inspect", tag]);
  ensure(
    DIGEST.test(loaded?.Id),
    "Loaded image has no immutable local identity.",
  );
  try {
    // Classic stores identify configs; containerd stores identify manifests/indices.
    // Read back Docker's actual bytes and verify the raw config, including layer
    // DiffIDs. Never substitute a local store ID for the published OCI digest.
    await command(["image", "save", loaded.Id], {
      output: join(directory, "runtime.tar"),
      timeout: 600_000,
    });
    const config = await transfer(
      tools,
      directory,
      ["inspect", "--config", "--raw", "docker-archive:/artifact/runtime.tar"],
      ["--network", "none"],
      true,
    );
    validateLoadedConfig(config, expectedConfig);
    validateLoadedIdentity(
      (await inspect(["image", "inspect", tag]))[0],
      loaded.Id,
    );
    return loaded.Id;
  } catch (error) {
    // Only remove our unique tag if it still names the image we imported.
    validateLoadedIdentity(
      (await inspect(["image", "inspect", tag]))[0],
      loaded.Id,
    );
    await command(["image", "rm", tag]);
    throw error;
  } finally {
    rmSync(archivePath);
  }
}
export async function roundtrip(tools, directory, artifact, resources) {
  const registry = await resources.container("registry", [
    "--tmpfs",
    "/var/lib/registry:rw,nosuid,size=256m",
    tools.registry,
  ]);
  await until(async () => {
    try {
      return (
        (
          await docker([
            "exec",
            registry,
            "wget",
            "-q",
            "-O",
            "-",
            "http://127.0.0.1:5000/v2/",
          ])
        ).trim() === "{}"
      );
    } catch {
      return false;
    }
  });
  await transport(
    tools,
    directory,
    [
      "copy",
      "--all",
      "--preserve-digests",
      "--dest-tls-verify=false",
      "oci:/artifact/layout",
      "docker://" + registry + ":5000/fleetiq:qualified",
    ],
    ["--network", resources.network],
  );
  await transport(
    tools,
    directory,
    [
      "copy",
      "--all",
      "--preserve-digests",
      "--src-tls-verify=false",
      "docker://" + registry + ":5000/fleetiq@" + artifact.digest,
      "oci:/artifact/roundtrip",
    ],
    ["--network", resources.network],
  );
}

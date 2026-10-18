/** Skopeo preserves the full image index and attestations; no custom registry client. */
import { join } from "node:path";
import { docker } from "./process.mjs";
import { until } from "./resources.mjs";
import { ensure } from "./policy.mjs";
function sandbox(directory, readOnly) {
  return [
    "--read-only",
    "--cap-drop",
    "ALL",
    "--security-opt",
    "no-new-privileges:true",
    "--env",
    "TMPDIR=/tmp",
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
export async function loadRuntime(tools, directory, tag, resources) {
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
  await docker(
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
  const archivePath = join(directory, "runtime.tar");
  // Do not use the daemon archive API: process-level tmpfs is only visible
  // inside the container. Docker exec streams raw bytes to a native host file.
  await docker(["exec", archive, "cat", "/tmp/runtime.tar"], {
    output: archivePath,
    timeout: 300_000,
  });
  console.log("Loading the exact runtime archive into Docker.");
  await docker(["load", "--input", archivePath], {
    timeout: 600_000,
  });
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

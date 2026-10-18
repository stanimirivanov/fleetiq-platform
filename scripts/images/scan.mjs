/** Scan the original OCI runtime manifest without Docker export or registry credentials. */
import {
  mkdirSync,
  writeFileSync,
  readFileSync,
  rmSync,
  existsSync,
} from "node:fs";
import { join } from "node:path";
import { docker, containerUser } from "./process.mjs";
import { validateScan, readJson, hash, ensure, DIGEST } from "./policy.mjs";
export function redactScan(scan) {
  // Keep every finding and severity; never upload matched credential bytes/code.
  for (const result of scan.Results ?? [])
    if (result.Secrets)
      result.Secrets = result.Secrets.map(({ RuleID, Severity, Category }) => ({
        RuleID,
        Severity,
        Category,
      }));
  return scan;
}
export function prepareScanLayout(directory, artifact) {
  ensure(
    DIGEST.test(artifact?.manifest) &&
      DIGEST.test(artifact?.config) &&
      artifact.runtime?.digest === artifact.manifest,
    "Scan requires the verified runtime manifest and config digests.",
  );
  // Trivy 0.76 cannot select a digest inside BuildKit's nested index. A standard
  // single-image entrypoint selects the verified descriptor without copying or
  // rewriting any blob. The published BuildKit layout remains untouched.
  const view = join(directory, "scan-layout");
  mkdirSync(join(view, "blobs"), { recursive: true });
  writeFileSync(
    join(view, "oci-layout"),
    JSON.stringify({ imageLayoutVersion: "1.0.0" }),
    { flag: "wx" },
  );
  writeFileSync(
    join(view, "index.json"),
    JSON.stringify({ schemaVersion: 2, manifests: [artifact.runtime] }),
    { flag: "wx" },
  );
  return view;
}
export async function scanRuntime(
  directory,
  tools,
  artifact,
  resources,
  execute = docker,
) {
  const view = prepareScanLayout(directory, artifact);
  const reports = join(directory, "reports");
  const privateOutput = join(directory, "private-scan");
  mkdirSync(privateOutput, { recursive: true, mode: 0o700 });
  // Match bind-mount ownership and keep raw secret-bearing output private.
  // Keep the large database on Linux scratch; only redacted JSON is retained.
  const scanner = await resources.container("scanner", [
    "--user",
    containerUser(),
    "--read-only",
    "--cap-drop",
    "ALL",
    "--security-opt",
    "no-new-privileges:true",
    "--tmpfs",
    "/tmp:rw,noexec,nosuid,size=512m",
    "--tmpfs",
    "/cache:rw,noexec,nosuid,size=4g,mode=1777",
    "--volume",
    view + ":/scan:ro",
    "--volume",
    join(directory, "layout", "blobs") + ":/scan/blobs:ro",
    "--volume",
    privateOutput + ":/private",
    "--entrypoint",
    "/bin/sleep",
    tools.scanner,
    "infinity",
  ]);
  const base = ["exec", scanner, "trivy", "--cache-dir", "/cache"];
  let failure;
  try {
    await execute(
      [
        ...base,
        "image",
        "--input",
        "/scan@" + artifact.manifest,
        "--platform",
        tools.platform,
        "--scanners",
        "vuln,secret",
        "--severity",
        "HIGH,CRITICAL",
        "--exit-code",
        "1",
        "--ignorefile",
        "/dev/null",
        "--list-all-pkgs",
        "--format",
        "json",
        "--output",
        "/private/raw.json",
        "--timeout",
        "10m",
        "--skip-version-check",
        "--no-progress",
      ],
      { capture: false, timeout: 660_000 },
    );
  } catch (error) {
    failure = error;
  } finally {
    const raw = join(privateOutput, "raw.json");
    if (existsSync(raw)) {
      try {
        writeFileSync(
          join(reports, "scan.json"),
          JSON.stringify(redactScan(readJson(raw)), null, 2) + "\n",
        );
      } finally {
        rmSync(raw);
      }
    }
  }
  if (failure) throw failure;
  const database = JSON.parse(
    await execute([...base, "version", "--format", "json"]),
  );
  validateScan(
    readJson(join(reports, "scan.json")),
    database,
    Date.now(),
    artifact.config,
  );
  writeFileSync(
    join(reports, "scanner.json"),
    JSON.stringify(database, null, 2) + "\n",
  );
  return {
    outcome: "passed",
    digest: "sha256:" + hash(readFileSync(join(reports, "scan.json"))),
    tool: tools.scanner,
    database_updated_at: database.VulnerabilityDB.UpdatedAt,
  };
}

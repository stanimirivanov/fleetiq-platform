/** Scan the exact runtime archive without a Docker socket or registry credentials. */
import {
  mkdirSync,
  writeFileSync,
  readFileSync,
  rmSync,
  existsSync,
} from "node:fs";
import { join } from "node:path";
import { docker, containerUser } from "./process.mjs";
import { validateScan, readJson, hash } from "./policy.mjs";
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
export async function scanRuntime(directory, tools, expectedConfig, resources) {
  const reports = join(directory, "reports");
  const privateOutput = join(directory, "private-scan");
  mkdirSync(privateOutput, { recursive: true, mode: 0o700 });
  // Without DAC capabilities, container root cannot read another UID's mode-0600
  // archive on Linux. Match the producer UID; retain private file permissions.
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
    directory + ":/artifact:ro",
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
    await docker(
      [
        ...base,
        "image",
        "--input",
        "/artifact/runtime.tar",
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
    await docker([...base, "version", "--format", "json"]),
  );
  validateScan(
    readJson(join(reports, "scan.json")),
    database,
    Date.now(),
    expectedConfig,
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

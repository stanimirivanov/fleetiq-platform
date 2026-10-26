import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { checkProducer, checkWorkflow } from "../workflow-policy.mjs";
import {
  validateDockerfile,
  validateTools,
  validateScan,
  validateQualification,
  CHECKS,
} from "../policy.mjs";
const read = (path) => readFileSync(path, "utf8").replaceAll("\r\n", "\n");
const rust = read("rust-toolchain.toml").match(/channel = "([^"]+)"/)[1];
test("current producer policy and CRLF workflow pass", () => {
  checkProducer();
  checkWorkflow(
    read(".github/publication.yml.template"),
    read(".github/workflows/publish-image.yml").replaceAll("\n", "\r\n"),
  );
});
test("publication permissions, branch, approval, identity, scan and artifact drift fail", () => {
  const template = read(".github/publication.yml.template");
  const workflow = read(".github/workflows/publish-image.yml");
  for (const [before, after] of [
    ["workflow_dispatch:", "pull_request:"],
    [
      "if: github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main'",
      "if: true",
    ],
    ["needs: authorize", "needs: unrelated"],
    ["persist-credentials: false", "persist-credentials: true"],
    ["contents: read", "contents: write"],
    ["packages: write", "packages: read"],
    ["node scripts/images/preflight.mjs", "node --version"],
    ["node scripts/images/qualify.mjs", "node --version"],
    ["node scripts/images/publish.mjs", "node --version"],
    ["if-no-files-found: error", "if-no-files-found: ignore"],
    ["cancel-in-progress: false", "cancel-in-progress: true"],
    ["node-version: '24'", "node-version: '25'"],
    ["registry: ghcr.io", "registry: unreviewed.example"],
  ]) {
    assert.ok(workflow.includes(before));
    assert.throws(() =>
      checkWorkflow(template, workflow.replaceAll(before, after)),
    );
  }
});
test("major action updates pass but hashes, minor tags, mixed refs and substitutions fail", () => {
  const template = read(".github/publication.yml.template");
  const workflow = read(".github/workflows/publish-image.yml");
  checkWorkflow(
    template,
    workflow.replaceAll("actions/checkout@v7", "actions/checkout@v8"),
  );
  for (const ref of ["v7.1.0", "v07", "main", "a".repeat(40), "v7 # bypass"])
    assert.throws(() =>
      checkWorkflow(
        template,
        workflow.replaceAll("actions/checkout@v7", "actions/checkout@" + ref),
      ),
    );
  assert.throws(() =>
    checkWorkflow(
      template,
      workflow.replace("actions/checkout@v7", "actions/checkout@v8"),
    ),
  );
  assert.throws(() =>
    checkWorkflow(
      template,
      workflow.replaceAll("actions/checkout@v7", "unreviewed/checkout@v7"),
    ),
  );
});
test("floating bases, wrong Rust, root user, unchecked default and unlocked build fail", () => {
  const dockerfile = read("Dockerfile");
  validateDockerfile(dockerfile, rust);
  for (const changed of [
    dockerfile.replace(/@sha256:[a-f0-9]{64}/g, ""),
    dockerfile.replace(`rust:${rust}`, "rust:0.0.0"),
    dockerfile.replace("USER 10001:10001", "USER 0:0"),
    dockerfile.replace(
      "gcr.io/distroless/cc-debian13:nonroot",
      "debian:trixie-slim",
    ),
    dockerfile.replace(
      '["/usr/local/bin/fleetiq-server", "healthcheck"]',
      '["true"]',
    ),
    dockerfile.replace('["serve", "--check-schema"]', '["serve"]'),
    dockerfile.replace("cargo build --locked", "cargo build"),
    dockerfile.replace("-p fleetiq-protocol-wasm --bins", ""),
    dockerfile.replace("cp target/release/fleetiq-protocol-decode /tmp/fleetiq-protocol-decode", ""),
    dockerfile.replace("COPY --from=builder /tmp/fleetiq-protocol-decode /usr/local/bin/fleetiq-protocol-decode", ""),
    dockerfile.replace("COPY --from=builder /tmp/fleetiq-protocol-admission /usr/local/bin/fleetiq-protocol-admission", ""),
  ]) {
    assert.notEqual(changed, dockerfile, "negative fixture must change the current Dockerfile");
    assert.throws(() => validateDockerfile(changed, rust));
  }
  const tools = JSON.parse(read("scripts/images/tools.json"));
  tools.scanner = "aquasec/trivy:latest";
  assert.throws(() => validateTools(tools));
});
const now = Date.parse("2026-10-18T12:00:00Z");
export function scanFixture() {
  return {
    SchemaVersion: 2,
    Metadata: { OS: { Family: "debian" } },
    Results: [
      { Class: "os-pkgs", Packages: [{ Name: "libc6" }], Vulnerabilities: [] },
    ],
  };
}
export function dbFixture() {
  return {
    Version: "0.76.0",
    VulnerabilityDB: {
      Version: 2,
      UpdatedAt: "2026-10-18T10:00:00Z",
      NextUpdate: "2026-10-18T16:00:00Z",
    },
  };
}
test("runtime scan rejects unfixed vulnerabilities, secrets, missing packages and stale databases", () => {
  validateScan(scanFixture(), dbFixture(), now);
  for (const severity of ["HIGH", "CRITICAL"]) {
    const scan = scanFixture();
    scan.Results[0].Vulnerabilities.push({
      Severity: severity,
      FixedVersion: "",
    });
    assert.throws(() => validateScan(scan, dbFixture(), now));
  }
  const secrets = scanFixture();
  secrets.Results[0].Secrets = [{ RuleID: "fixture" }];
  assert.throws(() => validateScan(secrets, dbFixture(), now));
  for (const updated of [
    "2026-10-16T10:00:00Z",
    "2026-10-19T10:00:00Z",
    "invalid",
  ]) {
    const db = dbFixture();
    db.VulnerabilityDB.UpdatedAt = updated;
    assert.throws(() => validateScan(scanFixture(), db, now));
  }
  assert.throws(() =>
    validateScan({ SchemaVersion: 2, Results: [] }, dbFixture(), now),
  );
  const scan = scanFixture();
  delete scan.Results[0].Packages;
  assert.throws(() => validateScan(scan, dbFixture(), now));
});
test("incomplete, cross-source and missing-check receipts cannot pass qualification", () => {
  const sha = "a".repeat(40);
  const digest = "sha256:" + "b".repeat(64);
  const receipt = {
    format: "fleetiq-image-qualification.v1",
    outcome: "passed",
    source: { revision: sha, repository: "stanimirivanov/fleetiq-platform" },
    platform: "linux/amd64",
    image: { digest, config: digest },
    checks: CHECKS,
    scan: { outcome: "passed", digest },
    inventory: { digest },
    sbom: { digest },
    provenance: { digest },
  };
  validateQualification(receipt, sha);
  for (const changed of [
    { ...receipt, outcome: "incomplete" },
    { ...receipt, checks: CHECKS.slice(0, -1) },
    { ...receipt, platform: "linux/arm64" },
    { ...receipt, source: { ...receipt.source, revision: "c".repeat(40) } },
  ])
    assert.throws(() => validateQualification(changed, sha));
});

test("transport requires upstream immutable namespace, version and digest", () => {
  const tools = JSON.parse(read("scripts/images/tools.json"));
  validateTools(tools);
  const digest = tools.transport.split("@")[1];
  for (const transport of [
    `quay.io/skopeo/stable:v1.22.3@${digest}`,
    `quay.io/containers/skopeo:v1.22.3@${digest}`,
    `quay.io/containers/skopeo:latest-immutable@${digest}`,
    "quay.io/containers/skopeo:v1.22.3-immutable",
  ]) assert.throws(() => validateTools({ ...tools, transport }), /immutable/);
});

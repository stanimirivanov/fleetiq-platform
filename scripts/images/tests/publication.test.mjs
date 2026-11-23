import test from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { inspectLayout } from "../oci.mjs";
import { validateEvidence } from "../publication.mjs";
import { hash, CHECKS } from "../policy.mjs";
import { layoutFixture } from "./fixtures.mjs";
function evidenceFixture() {
  const fixture = layoutFixture();
  const artifact = inspectLayout(fixture.layout, fixture.revision);
  const reports = join(fixture.directory, "reports");
  mkdirSync(reports);
  mkdirSync(join(fixture.directory, "migrations"));
  const sql = "CREATE SCHEMA example;\n";
  writeFileSync(
    join(fixture.directory, "migrations", "0001_create_example.sql"),
    sql,
  );
  const source = {
    repository: "stanimirivanov/fleetiq-platform",
    revision: fixture.revision,
    dirty: false,
    fingerprint: "sha256:" + "f".repeat(64),
  };
  const now = Date.parse("2026-10-18T12:00:00Z");
  const values = {
    migrations: {
      format: "fleetiq-migrations.v1",
      checksum_algorithm: "sha384",
      migrations: [
        {
          version: "1",
          description: "create example",
          checksum: hash(sql, "sha384"),
        },
      ],
    },
    scan: {
      SchemaVersion: 2,
      Metadata: { ImageID: artifact.config, OS: { Family: "debian" } },
      Results: [{ Class: "os-pkgs", Packages: [{ Name: "libc6" }] }],
    },
    scanner: {
      Version: "0.76.0",
      VulnerabilityDB: {
        Version: 2,
        UpdatedAt: "2026-10-18T10:00:00Z",
        NextUpdate: "2026-10-18T16:00:00Z",
      },
    },
  };
  for (const [name, value] of Object.entries(values))
    writeFileSync(join(reports, name + ".json"), JSON.stringify(value));
  for (const name of ["sbom", "provenance"])
    writeFileSync(join(reports, name + ".json"), artifact[name].bytes);
  const digest = (name) =>
    "sha256:" + hash(readFileSync(join(reports, name + ".json")));
  const receipt = {
    format: "fleetiq-image-qualification.v1",
    outcome: "passed",
    source,
    platform: "linux/amd64",
    image: {
      digest: artifact.digest,
      config: artifact.config,
      manifest: artifact.manifest,
    },
    checks: CHECKS,
    scan: { outcome: "passed", digest: digest("scan") },
    inventory: { digest: digest("migrations") },
    sbom: { digest: digest("sbom") },
    provenance: { digest: digest("provenance") },
  };
  const save = () =>
    writeFileSync(join(reports, "qualification.json"), JSON.stringify(receipt));
  save();
  return { ...fixture, source, reports, receipt, values, now, save };
}
test("publication consumes complete content-bound evidence from the identical clean source", () => {
  const fixture = evidenceFixture();
  try {
    assert.equal(
      validateEvidence(
        fixture.directory,
        fixture.source,
        fixture.now,
        fixture.directory,
      ).record.outcome,
      "passed",
    );
    assert.throws(() =>
      validateEvidence(
        fixture.directory,
        { ...fixture.source, dirty: true },
        fixture.now,
        fixture.directory,
      ),
    );
  } finally {
    fixture.cleanup();
  }
});
test("cross-artifact, tampered evidence, incomplete cleanup and wrong scan identities fail", () => {
  for (const change of [
    "artifact",
    "tamper",
    "incomplete",
    "scan-identity",
    "stale",
  ]) {
    const fixture = evidenceFixture();
    try {
      if (change === "artifact")
        fixture.receipt.image.digest = "sha256:" + "c".repeat(64);
      if (change === "tamper")
        writeFileSync(join(fixture.reports, "migrations.json"), "{}");
      if (change === "incomplete") fixture.receipt.checks = CHECKS.slice(0, -1);
      if (change === "scan-identity") {
        fixture.values.scan.Metadata.ImageID = "sha256:" + "d".repeat(64);
        writeFileSync(
          join(fixture.reports, "scan.json"),
          JSON.stringify(fixture.values.scan),
        );
        fixture.receipt.scan.digest =
          "sha256:" + hash(readFileSync(join(fixture.reports, "scan.json")));
      }
      fixture.save();
      assert.throws(() =>
        validateEvidence(
          fixture.directory,
          fixture.source,
          change === "stale" ? fixture.now + 86_400_000 : fixture.now,
          fixture.directory,
        ),
      );
    } finally {
      fixture.cleanup();
    }
  }
});

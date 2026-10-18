import test from "node:test";
import assert from "node:assert/strict";
import {
  mkdirSync,
  writeFileSync,
  existsSync,
  readFileSync,
  readdirSync,
} from "node:fs";
import { join } from "node:path";
import { scanRuntime } from "../scan.mjs";
import { layoutFixture } from "./fixtures.mjs";

function scannerFixture(f) {
  mkdirSync(join(f.directory, "reports"));
  const artifact = {
    manifest: f.image.digest,
    config: f.config.digest,
    runtime: f.image,
  };
  const report = {
    SchemaVersion: 2,
    Metadata: { ImageID: artifact.config, OS: { Family: "debian" } },
    Results: [{ Class: "os-pkgs", Packages: [{ Name: "libc6" }] }],
  };
  const db = {
    Version: "0.76.0",
    VulnerabilityDB: {
      Version: 2,
      UpdatedAt: new Date(Date.now() - 3600000).toISOString(),
      NextUpdate: new Date(Date.now() + 86400000).toISOString(),
    },
  };
  const calls = [];
  const execute = async (args) => {
    calls.push(args);
    if (args.includes("version")) return JSON.stringify(db);
    writeFileSync(
      join(f.directory, "private-scan/raw.json"),
      JSON.stringify(report),
    );
    return "";
  };
  return {
    artifact,
    report,
    calls,
    execute,
    resources: { container: async () => "scanner" },
    tools: { scanner: "fixture", platform: "linux/amd64" },
  };
}

test("scan uses the exact original OCI manifest and retains all security gates", async () => {
  const f = layoutFixture();
  try {
    const s = scannerFixture(f);
    const result = await scanRuntime(
      f.directory,
      s.tools,
      s.artifact,
      s.resources,
      s.execute,
    );
    assert.equal(result.outcome, "passed");
    const original = JSON.parse(readFileSync(join(f.layout, "index.json")));
    assert.notEqual(original.manifests[0].digest, s.artifact.manifest);
    const view = JSON.parse(
      readFileSync(join(f.directory, "scan-layout/index.json")),
    );
    assert.deepEqual(view, { schemaVersion: 2, manifests: [f.image] });
    assert.equal(
      JSON.parse(readFileSync(join(f.directory, "scan-layout/oci-layout")))
        .imageLayoutVersion,
      "1.0.0",
    );
    assert.deepEqual(readdirSync(join(f.directory, "scan-layout/blobs")), []);
    const args = s.calls[0];
    for (const [flag, value] of Object.entries({
      "--input": "/scan@" + s.artifact.manifest,
      "--platform": "linux/amd64",
      "--scanners": "vuln,secret",
      "--severity": "HIGH,CRITICAL",
      "--exit-code": "1",
      "--ignorefile": "/dev/null",
    }))
      assert.equal(args[args.indexOf(flag) + 1], value);
    assert.ok(!args.some((a) => a.includes("runtime.tar")));
    assert.ok(!args.includes("--ignore-unfixed"));
    assert.ok(!args.includes("--skip-db-update"));
    assert.ok(!existsSync(join(f.directory, "private-scan/raw.json")));
  } finally {
    f.cleanup();
  }
});

test("analysis errors, different images and malformed selectors cannot become a passed scan", async () => {
  for (const mode of [
    "analysis-error",
    "wrong-image",
    "invalid-manifest",
    "wrong-descriptor",
  ]) {
    const f = layoutFixture();
    try {
      const s = scannerFixture(f);
      if (mode === "invalid-manifest") s.artifact.manifest = "latest";
      if (mode === "wrong-descriptor")
        s.artifact.runtime = {
          ...s.artifact.runtime,
          digest: "sha256:" + "c".repeat(64),
        };
      if (mode === "wrong-image")
        s.report.Metadata.ImageID = "sha256:" + "b".repeat(64);
      const execute = async (args) => {
        const result = await s.execute(args);
        if (mode === "analysis-error" && !args.includes("version"))
          throw new Error("unexpected EOF");
        return result;
      };
      await assert.rejects(
        scanRuntime(f.directory, s.tools, s.artifact, s.resources, execute),
      );
      assert.ok(!existsSync(join(f.directory, "private-scan/raw.json")));
      if (mode === "analysis-error") assert.equal(s.calls.length, 1);
      if (["invalid-manifest", "wrong-descriptor"].includes(mode))
        assert.equal(s.calls.length, 0);
    } finally {
      f.cleanup();
    }
  }
});

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { inspectLayout } from "../oci.mjs";
import { validateInventory } from "../inventory.mjs";
import { hash } from "../policy.mjs";
import { run } from "../process.mjs";
import { layoutFixture } from "./fixtures.mjs";
test("OCI metadata binds runtime, subject, SBOM and provenance to one digest", () => {
  const f = layoutFixture();
  try {
    const artifact = inspectLayout(f.layout, f.revision);
    assert.equal(artifact.digest, f.index.digest);
    assert.equal(artifact.config, f.config.digest);
    assert.deepEqual(artifact.runtime, f.image);
    assert.throws(() => inspectLayout(f.layout, "b".repeat(40)));
  } finally {
    f.cleanup();
  }
});
test("corrupted OCI metadata and missing attestations cannot be accepted", () => {
  for (const mutation of [
    "corrupt",
    "subject",
    "missing-subject",
    "missing-sbom",
    "missing-rust-sbom",
    "missing-debian-sbom",
    "missing-source",
    "extra-platform",
  ]) {
    const f = layoutFixture();
    try {
      if (mutation === "corrupt")
        writeFileSync(
          join(f.layout, "blobs", "sha256", f.config.digest.slice(7)),
          "corruption",
        );
      else {
        let attestation = f.attestation;
        const manifests = [f.image];
        let layers = [f.sbom, f.buildSbom, f.provenance];
        if (mutation === "subject" || mutation === "missing-subject") {
          const statement = JSON.parse(
            readFileSync(
              join(f.layout, "blobs", "sha256", f.sbom.digest.slice(7)),
            ),
          );
          if (mutation === "missing-subject") statement.subject = [];
          else statement.subject[0].digest.sha256 = "b".repeat(64);
          layers = [f.blob(statement), f.buildSbom, f.provenance];
        }
        if (mutation === "missing-sbom") layers = [f.buildSbom, f.provenance];
        if (mutation === "missing-rust-sbom") layers = [f.sbom, f.provenance];
        if (mutation === "missing-debian-sbom")
          layers = [
            f.blob({
              predicateType: "https://spdx.dev/Document",
              subject: [{ digest: { sha256: f.image.digest.slice(7) } }],
              predicate: { name: "sbom", packages: [{ name: "root" }] },
            }),
            f.buildSbom,
            f.provenance,
          ];
        if (mutation === "missing-source") {
          const statement = JSON.parse(
            readFileSync(
              join(f.layout, "blobs", "sha256", f.provenance.digest.slice(7)),
            ),
          );
          statement.predicate.runDetails.metadata.buildkit_metadata.vcs = {};
          layers = [f.sbom, f.buildSbom, f.blob(statement)];
        }
        attestation = {
          ...attestation,
          ...f.blob({ schemaVersion: 2, layers }),
        };
        manifests.push(attestation);
        if (mutation === "extra-platform")
          manifests.push({
            ...f.image,
            platform: { os: "linux", architecture: "arm64" },
          });
        const index = f.blob({ schemaVersion: 2, manifests });
        writeFileSync(
          join(f.layout, "index.json"),
          JSON.stringify({ schemaVersion: 2, manifests: [index] }),
        );
      }
      assert.throws(() => inspectLayout(f.layout, f.revision));
    } finally {
      f.cleanup();
    }
  }
});
test("exact source migration bytes, decimal range and complete ordering are required", () => {
  const f = layoutFixture();
  try {
    mkdirSync(join(f.directory, "migrations"));
    const sql = "CREATE SCHEMA example;\n";
    writeFileSync(
      join(f.directory, "migrations", "0001_create_example.sql"),
      sql,
    );
    const inventory = {
      format: "fleetiq-migrations.v1",
      checksum_algorithm: "sha384",
      migrations: [
        {
          version: "1",
          description: "create example",
          checksum: hash(sql, "sha384"),
        },
      ],
    };
    validateInventory(inventory, f.directory);
    for (const entries of [
      [],
      [...inventory.migrations, ...inventory.migrations],
      [{ ...inventory.migrations[0], version: 1 }],
      [{ ...inventory.migrations[0], checksum: "b".repeat(96) }],
      [{ ...inventory.migrations[0], version: "9223372036854775808" }],
    ])
      assert.throws(() =>
        validateInventory({ ...inventory, migrations: entries }, f.directory),
      );
    writeFileSync(
      join(f.directory, "migrations", "0001_create_example.sql"),
      sql + "-- changed\n",
    );
    assert.throws(() => validateInventory(inventory, f.directory));
  } finally {
    f.cleanup();
  }
});
test("missing, unsuccessful and timed-out tools never return a clean result or echo output", async () => {
  assert.equal(
    (await run(process.execPath, ["-e", 'process.stdout.write("ok")'])).trim(),
    "ok",
  );
  await assert.rejects(run("fleetiq-nonexistent-image-tool", []));
  await assert.rejects(
    run(process.execPath, [
      "-e",
      'process.stderr.write("private-diagnostic");process.exit(9)',
    ]),
    (error) => !error.message.includes("private-diagnostic"),
  );
  await assert.rejects(
    run(process.execPath, ["-e", "setTimeout(()=>{},10000)"], { timeout: 30 }),
  );
});

test("binary output preserves bytes, refuses overwrite and rejects oversized or failed producers", async () => {
  const f = layoutFixture();
  try {
    await assert.rejects(
      run("fleetiq-nonexistent-binary-producer", [], {
        output: join(f.directory, "missing-producer"),
      }),
    );
    const destination = join(f.directory, "binary.tar");
    const bytes = Buffer.from([0, 255, 128, 13, 10]);
    await run(
      process.execPath,
      ["-e", "process.stdout.write(Buffer.from([0,255,128,13,10]))"],
      { output: destination },
    );
    assert.deepEqual(readFileSync(destination), bytes);
    await assert.rejects(
      run(process.execPath, ["-e", "process.stdout.write('replace')"], {
        output: destination,
      }),
    );
    assert.deepEqual(readFileSync(destination), bytes);
    await assert.rejects(
      run(process.execPath, ["-e", "process.stdout.write(Buffer.alloc(32))"], {
        output: join(f.directory, "oversized"),
        maxOutputBytes: 16,
      }),
    );
    await assert.rejects(
      run(process.execPath, ["-e", "process.exit(9)"], {
        output: join(f.directory, "failed"),
      }),
    );
    await assert.rejects(
      run(process.execPath, ["-e", "process.exit(0)"], {
        output: join(f.directory, "invalid-bound"),
        maxOutputBytes: Infinity,
      }),
    );
  } finally {
    f.cleanup();
  }
});

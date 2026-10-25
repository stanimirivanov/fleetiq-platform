import { hash } from "../policy.mjs";
import test from "node:test";
import { ImageToolFailure, failureDetails } from "../failure.mjs";
import assert from "node:assert/strict";
import { writeFileSync, existsSync, statSync } from "node:fs";
import { join } from "node:path";
import { layoutFixture } from "./fixtures.mjs";
import { scanRuntime } from "../scan.mjs";
import { containerUser } from "../process.mjs";
import {
  loadRuntime,
  preflightRuntimeExport,
  validateLoadedConfig,
  validateLoadedIdentity,
} from "../transport.mjs";

test("sandbox identity matches bind ownership without making private files public", () => {
  assert.equal(containerUser(1001, 123), "1001:123");
  assert.equal(containerUser(0, 0), "0:0");
  for (const uid of [-1, NaN, "1001", Infinity])
    assert.throws(() => containerUser(uid, 0));
  assert.equal(
    containerUser(),
    (process.getuid?.() ?? 0) + ":" + (process.getgid?.() ?? 0),
  );
});

test("config proof is byte exact and independent of Docker local identity", () => {
  const raw =
    '{"rootfs":{"diff_ids":["sha256:layer"]},"config":{"User":"10001"}}';
  const config = "sha256:" + hash(raw);
  const manifest = "sha256:" + "b".repeat(64);
  validateLoadedConfig(raw, config);
  for (const id of [config, manifest]) validateLoadedIdentity({ Id: id }, id);
  assert.throws(() => validateLoadedConfig(raw + "\n", config));
  assert.throws(() =>
    validateLoadedConfig(raw.replace("layer", "other"), config),
  );
  assert.throws(() => validateLoadedConfig(raw, manifest));
  assert.throws(() => validateLoadedIdentity({ Id: manifest }, config));
  assert.throws(() => validateLoadedIdentity({}, manifest));
  assert.throws(() => validateLoadedIdentity({ Id: undefined }, undefined));
});

test("load verifies Docker's read-back config for classic and manifest IDs and refuses drift", async () => {
  const raw = '{"rootfs":{"diff_ids":["sha256:layer"]}}';
  const config = "sha256:" + hash(raw);
  const manifest = "sha256:" + "b".repeat(64);
  for (const scenario of [
    "classic",
    "manifest",
    "wrong-config",
    "retagged",
    "save-failed",
  ]) {
    const f = layoutFixture();
    const id = scenario === "classic" ? config : manifest;
    const calls = [];
    let inspections = 0;
    const operations = {
      docker: async (args, options = {}) => {
        calls.push(args);
        if (
          scenario === "save-failed" &&
          args[0] === "image" &&
          args[1] === "save"
        )
          throw new Error("save failed");
        if (options.output) writeFileSync(options.output, "fixture archive");
        return "";
      },
      jsonDocker: async () => [
        { Id: ++inspections > 1 && scenario === "retagged" ? config : id },
      ],
      transport: async (_tools, _dir, args, extra, readOnly) => {
        assert.deepEqual(args, [
          "inspect",
          "--config",
          "--raw",
          "docker-archive:/artifact/runtime.tar",
        ]);
        assert.deepEqual(extra, ["--network", "none"]);
        assert.equal(readOnly, true);
        assert.ok(existsSync(join(f.directory, "runtime.tar")));
        return scenario === "wrong-config" ? raw + "\n" : raw;
      },
    };
    try {
      const result = loadRuntime(
        { transport: "fixture" },
        f.directory,
        "fixture:owned",
        {
          container: async (_suffix, args, network) => {
            assert.equal(args[args.indexOf("--user") + 1], containerUser());
            assert.ok(args.includes("--read-only"));
            assert.equal(args[args.indexOf("--cap-drop") + 1], "ALL");
            assert.equal(network, "none");
            return "fixture-archive";
          },
        },
        config,
        operations,
      );
      if (["classic", "manifest"].includes(scenario))
        assert.equal(await result, id);
      else await assert.rejects(result);
      assert.ok(!existsSync(join(f.directory, "import.tar")));
      const removed = calls.some(
        (args) => args[0] === "image" && args[1] === "rm",
      );
      assert.equal(removed, ["wrong-config", "save-failed"].includes(scenario));
      if (scenario !== "save-failed")
        assert.ok(calls.some((args) => args[1] === "save" && args[2] === id));
    } finally {
      f.cleanup();
    }
  }
});

test("scanner matches private evidence ownership and retains its hardened sandbox", async () => {
  const f = layoutFixture();
  try {
    await assert.rejects(
      scanRuntime(
        f.directory,
        { scanner: "fixture" },
        { manifest: f.image.digest, config: f.config.digest, runtime: f.image },
        {
          container: async (_suffix, args) => {
            assert.equal(args[args.indexOf("--user") + 1], containerUser());
            assert.ok(args.includes("--read-only"));
            assert.equal(args[args.indexOf("--cap-drop") + 1], "ALL");
            assert.ok(
              args.includes(join(f.directory, "scan-layout") + ":/scan:ro"),
            );
            assert.ok(
              args.includes(
                join(f.directory, "layout", "blobs") + ":/scan/blobs:ro",
              ),
            );
            assert.ok(args.includes("no-new-privileges:true"));
            assert.ok(
              args.includes("/cache:rw,noexec,nosuid,size=4g,mode=1777"),
            );
            if (process.platform !== "win32")
              assert.equal(
                statSync(join(f.directory, "private-scan")).mode & 0o777,
                0o700,
              );
            throw new Error("fixture stopped before scan");
          },
        },
      ),
      /fixture stopped before scan/,
    );
  } finally {
    f.cleanup();
  }
});


const remoteManifest = JSON.stringify({ schemaVersion: 2, config: { digest: "sha256:" + "a".repeat(64) }, layers: [] });
const preflightTools = { transport: "quay.io/containers/skopeo:v1.22.3-immutable@sha256:" + hash(remoteManifest) };

test("export preflight verifies remote bytes before starting the identical isolated helper", async () => {
  const calls = [];
  await preflightRuntimeExport(preflightTools, "fixture", async (args, options) => {
    calls.push(args);
    if (args[0] === "buildx") {
      assert.deepEqual(args, ["buildx", "imagetools", "inspect", "--raw", preflightTools.transport]);
      assert.equal(options.timeout, 60_000);
      return remoteManifest;
    }
    assert.equal(calls.length, 2, "remote verification must precede sandbox startup");
    assert.equal(args[0], "run");
    assert(args.includes("--rm"));
    assert.equal(args[args.indexOf("--pull") + 1], "always");
    assert(args.includes("--read-only"));
    assert.equal(args[args.indexOf("--user") + 1], containerUser());
    assert.equal(args[args.indexOf("--cap-drop") + 1], "ALL");
    assert(args.includes("no-new-privileges:true"));
    assert.equal(args[args.indexOf("--network") + 1], "none");
    assert.equal(args[args.indexOf("--entrypoint") + 1], "/usr/bin/sleep");
    assert.deepEqual(args.slice(-2), [preflightTools.transport, "0"]);
    assert(args.includes("fixture:/artifact:ro"));
    assert(!args.some((argument) => argument.includes("docker.sock")));
    assert.equal(options.timeout, 120_000);
  });
  assert.equal(calls.length, 2);
});

test("cached images cannot hide remote failure and sandbox refusal is not retried", async () => {
  for (const failedCall of [1, 2]) {
    let calls = 0;
    await assert.rejects(preflightRuntimeExport(preflightTools, "fixture", async () => {
      calls++;
      if (calls === failedCall) throw new ImageToolFailure("run", 125, false, "image-unavailable");
      return remoteManifest;
    }), error => {
      assert.equal(failureDetails(error).prerequisite, failedCall === 1 ? "transport-remote-manifest" : "transport-sandbox");
      assert.equal(failureDetails(error).category, "image-unavailable");
      assert.equal(failureDetails(error).exitCode, 125);
      return true;
    });
    assert.equal(calls, failedCall, "no retry or cached fallback may hide a failed prerequisite");
  }
});

test("wrong digest, invalid JSON and malformed manifests stop before acquisition", async () => {
  for (const raw of [remoteManifest + " ", "not-json", JSON.stringify({ schemaVersion: 1 }), "x".repeat(2 * 1024 * 1024 + 1)]) {
    const tools = { transport: "quay.io/containers/skopeo:v1.22.3-immutable@sha256:" + hash(raw) };
    if (raw === remoteManifest + " ") tools.transport = preflightTools.transport;
    let calls = 0;
    await assert.rejects(preflightRuntimeExport(tools, "fixture", async () => { calls++; return raw; }), error => {
      assert.equal(failureDetails(error).prerequisite, "transport-manifest-validation");
      return true;
    });
    assert.equal(calls, 1);
  }
});

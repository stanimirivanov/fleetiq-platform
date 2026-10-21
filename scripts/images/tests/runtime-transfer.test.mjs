import test from "node:test";
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
import { hash } from "../policy.mjs";

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


test("export preflight starts the identical isolated helper and propagates refusal", async () => {
  const tools = { transport: "fixture@sha256:immutable" };
  for (const refuse of [false, true]) {
    const calls = [];
    const execute = async (args, options) => {
      calls.push(args);
      assert.equal(args[0], "run");
      assert(args.includes("--rm"));
      assert(args.includes("--read-only"));
      assert.equal(args[args.indexOf("--user") + 1], containerUser());
      assert.equal(args[args.indexOf("--cap-drop") + 1], "ALL");
      assert(args.includes("no-new-privileges:true"));
      assert.equal(args[args.indexOf("--network") + 1], "none");
      assert.equal(args[args.indexOf("--entrypoint") + 1], "/usr/bin/sleep");
      assert.deepEqual(args.slice(-2), [tools.transport, "0"]);
      assert(args.includes("fixture:/artifact:ro"));
      assert(!args.some((argument) => argument.includes("docker.sock")));
      assert.equal(options.timeout, 120_000);
      if (refuse) throw new Error("fixture helper refused");
    };
    if (refuse)
      await assert.rejects(preflightRuntimeExport(tools, "fixture", execute), /fixture helper refused/);
    else await preflightRuntimeExport(tools, "fixture", execute);
    assert.equal(calls.length, 1, "preflight must not silently retry a failing sandbox");
  }
});

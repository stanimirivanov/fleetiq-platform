import test from "node:test";
import assert from "node:assert/strict";
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  rmSync,
  symlinkSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { localBuildCache } from "../source.mjs";
test("explicit cache optimization cannot admit outside paths, links or missing indexes", () => {
  const root = mkdtempSync(join(tmpdir(), "fleetiq-cache-test-"));
  try {
    const target = join(root, "target");
    const cache = join(target, "cache");
    mkdirSync(cache, { recursive: true });
    assert.deepEqual(localBuildCache(root, undefined), []);
    assert.throws(() => localBuildCache(root, root));
    assert.throws(() => localBuildCache(root, cache));
    writeFileSync(
      join(cache, "index.json"),
      JSON.stringify({ schemaVersion: 2 }),
    );
    assert.deepEqual(localBuildCache(root, cache), [
      "--cache-from",
      "type=local,src=" + cache,
    ]);
    const link = join(target, "link");
    symlinkSync(cache, link, 'dir');
    assert.throws(() => localBuildCache(root, link));
    rmSync(link);
    writeFileSync(
      join(cache, "index.json"),
      JSON.stringify({ schemaVersion: 1 }),
    );
    assert.throws(() => localBuildCache(root, cache));
  } finally {
    rmSync(root, { recursive: true });
  }
});

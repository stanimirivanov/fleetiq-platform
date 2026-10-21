import test from "node:test";
import assert from "node:assert/strict";
import { run } from "../process.mjs";
import { diagnosticCategory, failureDetails } from "../failure.mjs";

for (const [message, category] of [
  ["failed to register layer: no space left on device", "storage-exhausted"],
  ["Cannot connect to the Docker daemon", "daemon-unavailable"],
  ["pull access denied: authentication required", "registry-authentication"],
  ["TLS handshake timeout", "network-or-timeout"],
  ["manifest unknown", "image-unavailable"],
  ["no such manifest", "image-unavailable"],
  ["invalid mount config for type bind", "invalid-mount"],
  ['exec: "/fixture": stat /fixture: no such file or directory', "entrypoint-unavailable"],
  ["permission denied", "permission-denied"],
  ["unrecognized private diagnostic", "command-failed"],
]) {
  test(`tool diagnostic has a finite safe classification: ${category}: ${message}`, () => {
    assert.equal(diagnosticCategory(message), category);
  });
}

test("real failed tool retains category and exit code but no child text or arguments", async () => {
  const privateText = "fixture-private-diagnostic";
  const diagnostic = "failed to register layer: no space left on device " + privateText;
  await assert.rejects(
    run(process.execPath, ["-e", `process.stderr.write(${JSON.stringify(diagnostic)});process.exit(125)`]),
    (error) => {
      assert.deepEqual(failureDetails(error), {
        operation: "tool", exitCode: 125, bounded: false, category: "storage-exhausted",
      });
      assert(!error.message.includes(privateText));
      assert(!error.message.includes("process.stderr"));
      assert(!JSON.stringify(error).includes(privateText));
      assert(error.message.includes("category=storage-exhausted"));
      return true;
    },
  );
});

test("missing tools and bounded processes cannot fabricate a successful numeric exit", async () => {
  await assert.rejects(run("fleetiq-nonexistent-process-fixture", []), (error) => {
    assert.equal(failureDetails(error).category, "tool-unavailable");
    assert.equal(failureDetails(error).exitCode, null);
    return true;
  });
  await assert.rejects(
    run(process.execPath, ["-e", "setTimeout(()=>{},10000)"], { timeout: 30 }),
    (error) => failureDetails(error).bounded === true,
  );
  assert.deepEqual(failureDetails(new Error("fixture-private-diagnostic")), { category: "operation-failed" });
});

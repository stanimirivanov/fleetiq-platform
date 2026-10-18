import test from "node:test";
import assert from "node:assert/strict";
import { redactScan } from "../scan.mjs";
import { validateScan } from "../policy.mjs";
test("secret evidence redaction retains rejection without exposing match or code", () => {
  const scan = {
    SchemaVersion: 2,
    Metadata: { OS: { Family: "debian" } },
    Results: [
      {
        Class: "os-pkgs",
        Packages: [{ Name: "libc6" }],
        Secrets: [
          {
            RuleID: "fixture",
            Severity: "HIGH",
            Match: "private-value",
            Code: { Lines: [{ Content: "private-value" }] },
          },
        ],
      },
    ],
  };
  const redacted = redactScan(scan);
  assert.equal(redacted.Results[0].Secrets.length, 1);
  assert.equal(redacted.Results[0].Secrets[0].RuleID, "fixture");
  assert.ok(!JSON.stringify(redacted).includes("private-value"));
  assert.throws(() => validateScan(redacted, {}, Date.now()));
});

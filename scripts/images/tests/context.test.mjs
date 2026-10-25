import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { validateContext } from "../workflow-policy.mjs";
test("context cannot add operator files, admit Git metadata or omit secret exclusions", () => {
  const text = readFileSync(".dockerignore", "utf8").replaceAll("\r\n", "\n");
  validateContext(text);
  for (const changed of [
    text + "!.env\n",
    text.replace("!examples/custom-protocol/**", "!examples/**"),
    text.replace("!packages/teltonika/**", "!packages/**"),
    text.replace("!packages/teltonika/**", ""),
    text.replace("**/.git", ""),
    text.replace("**/.env\n", ""),
    text.replace("!crates/**", "!**"),
  ])
    assert.throws(() => validateContext(changed));
});

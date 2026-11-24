import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

const revision = "a".repeat(40);
const head = "b".repeat(40);
const tree = "c".repeat(40);
const repository = "stanimirivanov/fleetiq-platform";
const run = {
  id: 10,
  run_attempt: 1,
  head_sha: head,
  event: "pull_request",
  status: "completed",
  conclusion: "success",
  path: ".github/workflows/ci.yml",
  display_title: `FleetIQ PR 304 from ${head} to main at ${revision}`,
};
const pull = {
  number: 304,
  merged_at: "2026-10-18T12:00:00Z",
  merge_commit_sha: revision,
  base: { ref: "main" },
  head: { sha: head, repo: { full_name: repository } },
};
const main = { sha: revision, commit: { tree: { sha: tree } } };
const tested = { sha: revision, tree: { sha: tree } };
const admin = { username: "owner", permission: "admin", role_name: "admin" };
const maintainer = { username: "reviewer", permission: "write", role_name: "maintain" };

function invoke(env, responses = {}, entryPoint = "authorization") {
  const directory = mkdtempSync(join(tmpdir(), "fleetiq-authorization-"));
  try {
    const routes = {
      "commits/main": main,
      "collaborators/owner/permission": { ...admin, user: { login: admin.username } },
      "collaborators/reviewer/permission": {
        ...maintainer,
        user: { login: maintainer.username },
      },
      [`commits/${revision}/pulls?per_page=100`]: [pull],
      [`actions/workflows/ci.yml/runs?event=pull_request&head_sha=${head}&per_page=100`]: {
        workflow_runs: [run],
      },
      [`git/commits/${revision}`]: tested,
      ...responses,
    };
    const cli = join(directory, "curl");
    writeFileSync(join(directory, "responses.json"), JSON.stringify(routes));
    writeFileSync(cli, `#!/usr/bin/env bash
set -eu
read -r header
[[ $header == "Authorization: Bearer synthetic-authorization-fixture" && $1 == -q ]]
url=\${!#}
prefix=https://api.github.com/repos/${repository}/
[[ $url == "$prefix"* ]]
jq -ce --arg endpoint "\${url#"$prefix"}" '
  .[$endpoint] | if . == null then error("unknown fixture path")
              elif type == "object" and .fixture_status == 302 then {}, 302
              else ., 200 end
' "${directory}/responses.json"
`, { mode: 0o700 });
    const args = entryPoint === "authorization"
      ? ["bash", "scripts/images/authorization.sh"]
      : [process.execPath, "scripts/images/publish.mjs"];
    return spawnSync(args[0], args.slice(1), {
      encoding: "utf8",
      timeout: 10000,
      env: {
        ...process.env,
        PATH: directory + ":" + process.env.PATH,
        GITHUB_EVENT_NAME: "workflow_dispatch",
        GITHUB_REPOSITORY: repository,
        GITHUB_REF: "refs/heads/main",
        GITHUB_SHA: revision,
        FLEETIQ_SOURCE_REVISION: revision,
        GITHUB_RUN_ID: "123",
        GITHUB_RUN_ATTEMPT: "1",
        GITHUB_ACTOR: "owner",
        GITHUB_TRIGGERING_ACTOR: "reviewer",
        GITHUB_TOKEN: "synthetic-authorization-fixture",
        ...env,
      },
    });
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

function refused(result, reason) {
  assert.equal(result.status, 1, result.stderr);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, reason);
  assert(!result.stderr.includes("synthetic-authorization-fixture"));
}

test("publication authorization emits the existing receipt for the exact tested tree", () => {
  const result = invoke({});
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), {
    format: "fleetiq-image-authorization.v1", event: "workflow_dispatch",
    repository, ref: "refs/heads/main", workflowRevision: revision, revision,
    run: "123", attempt: "1", actor: "owner", triggeringActor: "reviewer",
    pull_request: 304, ci_run: 10, tested_head: head, tested_revision: revision, tested_tree: tree,
    actor_authorization: { actor: "owner", role: "admin", permission: "admin" },
    triggering_authorization: { actor: "reviewer", role: "maintain", permission: "write" },
  });
});

test("publication requires manual dispatch, canonical main, and exact execution identity", () => {
  for (const env of [
    { GITHUB_EVENT_NAME: "pull_request" }, { GITHUB_REF: "refs/tags/main" },
    { GITHUB_REF: "refs/heads/feature" }, { GITHUB_REPOSITORY: "other/platform" },
    { FLEETIQ_SOURCE_REVISION: "main" }, { GITHUB_SHA: head },
    { GITHUB_RUN_ATTEMPT: "0" }, { GITHUB_TOKEN: "" },
  ]) refused(invoke(env), /Publication|Invalid publication|GitHub authorization/u);
  refused(invoke({}, { "commits/main": { ...main, sha: head } }), /exact current main/u);
});

test("a rerun requires the current maintainer role and the correct GitHub account", () => {
  for (const role of ["write", "read", "triage", "none"]) {
    refused(invoke({}, {
      "collaborators/reviewer/permission": {
        user: { login: "reviewer" }, permission: role, role_name: role,
      },
    }), /administrator\/maintainer/u);
  }
  refused(invoke({}, { "collaborators/owner/permission": {
    user: { login: "other" }, permission: "admin", role_name: "admin",
  } }), /administrator\/maintainer/u);
});

test("unmerged sources, fork heads, stale success and mismatched trees fail closed", () => {
  const pulls = `commits/${revision}/pulls?per_page=100`;
  refused(invoke({}, { [pulls]: [{ ...pull, merged_at: null }] }), /merged repository PR/u);
  refused(invoke({}, { [pulls]: [
    { ...pull, head: { ...pull.head, repo: { full_name: "other/platform" } } },
  ] }), /merged repository PR/u);
  const runs = `actions/workflows/ci.yml/runs?event=pull_request&head_sha=${head}&per_page=100`;
  for (const workflow_runs of [
    [], [{ ...run, conclusion: "failure" }], [{ ...run, status: "in_progress" }],
    [run, { ...run, run_attempt: 2, conclusion: "failure" }],
    [run, { ...run, id: 11, conclusion: "failure" }],
    [{ ...run, path: ".github/workflows/other.yml" }],
    [{ ...run, display_title: `FleetIQ PR 999 from ${head} to main at ${revision}` }],
    [{ ...run, head_sha: revision }],
  ]) refused(invoke({}, { [runs]: { workflow_runs } }), /latest PR CI/u);
  refused(invoke({}, { [`git/commits/${revision}`]: { ...tested, tree: { sha: head } } }), /exact passing CI checkout/u);
});

test("redirected GitHub responses cannot authorize source or expose headers", () => {
  refused(invoke({}, { "commits/main": { fixture_status: 302 } }), /GitHub prerequisite lookup failed/u);
});

test("final publication stops at stale authorization and classifies the refusal", () => {
  refused(invoke({}, { "commits/main": { ...main, sha: head } }, "publish"), /publication-source-stale/u);
});

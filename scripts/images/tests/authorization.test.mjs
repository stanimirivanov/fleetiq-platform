import test from "node:test";
import assert from "node:assert/strict";
import {
  validateRequest,
  validateMaintainer,
  validateMergedCi,
  validateTestedTree,
} from "../authorization.mjs";
const revision = "a".repeat(40);
const head = "b".repeat(40);
const request = {
  event: "workflow_dispatch",
  repository: "stanimirivanov/fleetiq-platform",
  ref: "refs/heads/main",
  revision,
  workflowRevision: revision,
  run: "123",
  attempt: "1",
};
const pulls = [
  {
    number: 304,
    merged_at: "2026-10-18T12:00:00Z",
    merge_commit_sha: revision,
    base: { ref: "main" },
    head: { sha: head, repo: { full_name: request.repository } },
  },
];
const passed = {
  id: 10,
  head_sha: head,
  event: "pull_request",
  status: "completed",
  conclusion: "success",
  path: ".github/workflows/ci.yml",
  display_title: "FleetIQ PR 304 from " + head + " to main at " + revision,
};
test("only exact manual current-main publication requests qualify", () => {
  validateRequest(request, { sha: revision });
  for (const changed of [
    { ...request, event: "pull_request" },
    { ...request, ref: "refs/tags/main" },
    { ...request, ref: "refs/heads/feature" },
    { ...request, repository: "other/platform" },
    { ...request, revision: "main" },
    { ...request, workflowRevision: head },
    { ...request, attempt: "0" },
  ])
    assert.throws(() => validateRequest(changed, { sha: revision }));
  assert.throws(() => validateRequest(request, { sha: head }));
});
test("read/write/triage access and a substituted actor cannot authorize publication or reruns", () => {
  validateMaintainer(
    { permission: "admin", role_name: "admin", user: { login: "maintainer" } },
    "maintainer",
  );
  validateMaintainer(
    {
      permission: "write",
      role_name: "maintain",
      user: { login: "maintainer" },
    },
    "maintainer",
  );
  for (const role of ["write", "read", "triage", "none"])
    assert.throws(() =>
      validateMaintainer(
        { permission: role, role_name: role, user: { login: "maintainer" } },
        "maintainer",
      ),
    );
  assert.throws(() =>
    validateMaintainer(
      { permission: "admin", role_name: "admin", user: { login: "other" } },
      "maintainer",
    ),
  );
});
test("unmerged sources, stale success, missing CI and other workflows fail closed", () => {
  assert.equal(
    validateMergedCi(pulls, { workflow_runs: [passed] }, revision).ci_run,
    10,
  );
  for (const workflow_runs of [
    [],
    [{ ...passed, status: "in_progress", conclusion: null }],
    [{ ...passed, conclusion: "failure" }],
    [passed, { ...passed, id: 11, conclusion: "failure" }],
    [{ ...passed, path: ".github/workflows/other.yml" }],
    [{ ...passed, display_title: "FleetIQ PR 999 from " + head + " to main" }],
    [
      {
        ...passed,
        display_title: "FleetIQ PR 304 from " + head + " to development",
      },
    ],
    [{ ...passed, head_sha: revision }],
  ])
    assert.throws(() => validateMergedCi(pulls, { workflow_runs }, revision));
  assert.throws(() =>
    validateMergedCi(
      [{ ...pulls[0], merged_at: null }],
      { workflow_runs: [passed] },
      revision,
    ),
  );
  assert.throws(() =>
    validateMergedCi(pulls, { workflow_runs: [passed] }, head),
  );
});

test("passing CI must identify the exact checkout and tree eventually merged", () => {
  const tree = "c".repeat(40);
  const main = { commit: { tree: { sha: tree } } };
  const tested = { sha: revision, tree: { sha: tree } };
  assert.equal(validateTestedTree(main, tested, revision), tree);
  assert.throws(() => validateTestedTree(main, tested, head));
  assert.throws(() =>
    validateTestedTree(main, { ...tested, tree: { sha: head } }, revision),
  );
  assert.throws(() => validateTestedTree({}, tested, revision));
  assert.throws(() =>
    validateMergedCi(
      pulls,
      {
        workflow_runs: [
          {
            ...passed,
            display_title: "FleetIQ PR 304 from " + head + " to main",
          },
        ],
      },
      revision,
    ),
  );
});

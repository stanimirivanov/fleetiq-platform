/** Read-only checks bind human maintainer authorization to the exact reviewed source. */
import { ensure, REPOSITORY, SHA } from "./policy.mjs";
export function validateRequest(request, main) {
  ensure(
    request.event === "workflow_dispatch" &&
      request.repository === REPOSITORY &&
      request.ref === "refs/heads/main",
    "Publication requires a manual main-branch request.",
  );
  ensure(
    SHA.test(request.revision) &&
      request.revision === request.workflowRevision &&
      main.sha === request.revision,
    "Publication requires the exact current main revision.",
  );
  ensure(
    /^[1-9][0-9]*$/.test(request.run) && /^[1-9][0-9]*$/.test(request.attempt),
    "Invalid publication execution identity.",
  );
}
export function validateMaintainer(permission, actor) {
  ensure(
    permission.user?.login === actor &&
      (permission.permission === "admin" ||
        permission.role_name === "maintain"),
    "Only a current repository administrator/maintainer may publish.",
  );
  return {
    actor,
    role: permission.role_name,
    permission: permission.permission,
  };
}
export function validateMergedCi(pulls, runs, revision) {
  const pr = pulls.find(
    (x) =>
      x.merged_at &&
      x.merge_commit_sha === revision &&
      x.base?.ref === "main" &&
      x.head?.repo?.full_name === REPOSITORY,
  );
  ensure(pr, "Source must be a merged repository PR targeting main.");
  const latest = runs.workflow_runs
    ?.filter((x) => x.head_sha === pr.head.sha && x.event === "pull_request")
    .sort((a, b) => b.id - a.id || b.run_attempt - a.run_attempt)[0];
  const prefix =
    "FleetIQ PR " + pr.number + " from " + pr.head.sha + " to main at ";
  const testedRevision = latest?.display_title?.startsWith(prefix)
    ? latest.display_title.slice(prefix.length)
    : "";
  ensure(
    latest?.status === "completed" &&
      latest.conclusion === "success" &&
      latest.path === ".github/workflows/ci.yml" &&
      SHA.test(testedRevision),
    "The latest PR CI run must have passed.",
  );
  return {
    pull_request: pr.number,
    ci_run: latest.id,
    tested_head: pr.head.sha,
    tested_revision: testedRevision,
  };
}
/** The PR merge checkout must contain exactly the eventual merged source bytes. */
export function validateTestedTree(main, tested, expectedRevision) {
  ensure(
    tested.sha === expectedRevision &&
      SHA.test(main.commit?.tree?.sha) &&
      main.commit.tree.sha === tested.tree?.sha,
    "Merged source tree differs from the exact passing CI checkout.",
  );
  return main.commit.tree.sha;
}
export async function github(path) {
  ensure(
    process.env.GITHUB_TOKEN,
    "GitHub authorization token is unavailable.",
  );
  const response = await fetch(
    "https://api.github.com/repos/" + REPOSITORY + "/" + path,
    {
      headers: {
        Authorization: "Bearer " + process.env.GITHUB_TOKEN,
        Accept: "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
      },
      redirect: "error",
      signal: AbortSignal.timeout(15_000),
    },
  );
  ensure(
    response.ok,
    "GitHub prerequisite lookup failed (HTTP " + response.status + ").",
  );
  return await response.json();
}
export async function authorize() {
  const request = {
    event: process.env.GITHUB_EVENT_NAME,
    repository: process.env.GITHUB_REPOSITORY,
    ref: process.env.GITHUB_REF,
    workflowRevision: process.env.GITHUB_SHA,
    revision: process.env.FLEETIQ_SOURCE_REVISION,
    run: process.env.GITHUB_RUN_ID,
    attempt: process.env.GITHUB_RUN_ATTEMPT,
    actor: process.env.GITHUB_ACTOR,
    triggeringActor:
      process.env.GITHUB_TRIGGERING_ACTOR ?? process.env.GITHUB_ACTOR,
  };
  validateRequest(request, { sha: request.workflowRevision });
  ensure(
    /^[a-zA-Z0-9][a-zA-Z0-9-]{0,38}$/.test(request.actor) &&
      /^[a-zA-Z0-9][a-zA-Z0-9-]{0,38}$/.test(request.triggeringActor),
    "Unsupported publication actor.",
  );
  const [main, actorPermission, triggerPermission, pulls] = await Promise.all([
    github("commits/main"),
    github("collaborators/" + request.actor + "/permission"),
    github("collaborators/" + request.triggeringActor + "/permission"),
    github("commits/" + request.revision + "/pulls?per_page=100"),
  ]);
  validateRequest(request, main);
  const actor = validateMaintainer(actorPermission, request.actor);
  const trigger = validateMaintainer(
    triggerPermission,
    request.triggeringActor,
  );
  const pr = pulls.find(
    (x) =>
      x.merged_at &&
      x.merge_commit_sha === request.revision &&
      x.base?.ref === "main",
  );
  ensure(pr && SHA.test(pr.head?.sha), "No eligible merged source PR.");
  const runs = await github(
    "actions/workflows/ci.yml/runs?event=pull_request&head_sha=" +
      pr.head.sha +
      "&per_page=100",
  );
  const ci = validateMergedCi(pulls, runs, request.revision);
  const tested = await github("git/commits/" + ci.tested_revision);
  const testedTree = validateTestedTree(main, tested, ci.tested_revision);
  return {
    format: "fleetiq-image-authorization.v1",
    ...request,
    ...ci,
    tested_tree: testedTree,
    actor_authorization: actor,
    triggering_authorization: trigger,
  };
}

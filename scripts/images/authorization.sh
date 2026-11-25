#!/usr/bin/env bash
# Print one receipt only after checking the current GitHub source and actors.
set -euo pipefail

fail() {
    printf '%s\n' "$1" >&2
    exit 1
}

repository=stanimirivanov/fleetiq-platform
revision=${FLEETIQ_SOURCE_REVISION:-}
actor=${GITHUB_ACTOR:-}
trigger=${GITHUB_TRIGGERING_ACTOR-$actor}
sha_pattern='^[a-f0-9]{40}$'
actor_pattern='^[a-zA-Z0-9][a-zA-Z0-9-]{0,38}$'

[[ ${GITHUB_EVENT_NAME:-} == workflow_dispatch && ${GITHUB_REPOSITORY:-} == "$repository" &&
    ${GITHUB_REF:-} == refs/heads/main ]] || fail 'Publication requires a manual main-branch request.'
[[ $revision =~ $sha_pattern && $revision == "${GITHUB_SHA:-}" ]] ||
    fail 'Publication requires the exact current main revision.'
[[ ${GITHUB_RUN_ID:-} =~ ^[1-9][0-9]*$ && ${GITHUB_RUN_ATTEMPT:-} =~ ^[1-9][0-9]*$ ]] ||
    fail 'Invalid publication execution identity.'
[[ $actor =~ $actor_pattern && $trigger =~ $actor_pattern ]] ||
    fail 'Unsupported publication actor.'
[[ ${GITHUB_TOKEN:-} =~ ^[a-zA-Z0-9_-]+$ ]] || fail 'GitHub authorization token is unavailable or malformed.'
command -v curl >/dev/null && command -v jq >/dev/null ||
    fail 'Publication authorization requires curl and jq.'

github() {
    local response
    # Disable user curl configuration and require HTTP 200 from the original
    # endpoint. Feed the bearer header on stdin so it never appears in argv.
    response=$(
        printf 'Authorization: Bearer %s\n' "$GITHUB_TOKEN" |
            curl -q --silent --proto '=https' --max-time 15 --max-filesize 33554432 \
                --header @- --header 'Accept: application/vnd.github+json' \
                --header 'X-GitHub-Api-Version: 2022-11-28' \
                --write-out $'\n%{http_code}' "https://api.github.com/repos/$repository/$1"
    ) || fail 'GitHub prerequisite lookup failed.'
    [[ ${response##*$'\n'} == 200 ]] || fail 'GitHub prerequisite lookup failed.'
    printf '%s\n' "${response%$'\n'*}"
}

maintainer() {
    github "collaborators/$1/permission" |
        jq -ce --arg actor "$1" '
            select(.user.login == $actor and (.permission == "admin" or .role_name == "maintain"))
            | {actor: $actor, role: .role_name, permission}
        ' 2>/dev/null || fail 'Only a current repository administrator/maintainer may publish.'
}

tree=$(github commits/main | jq -er --arg revision "$revision" '
    select(.sha == $revision) | .commit.tree.sha | strings | select(test("^[a-f0-9]{40}$"))
' 2>/dev/null) || fail 'Publication requires the exact current main revision and Git tree.'

actor_authorization=$(maintainer "$actor")
triggering_authorization=$(maintainer "$trigger")

pull=$(github "commits/$revision/pulls?per_page=100" |
    jq -ce --arg revision "$revision" --arg repository "$repository" '
        first(.[] | select(
            (.merged_at | type == "string" and length > 0)
            and .merge_commit_sha == $revision and .base.ref == "main"
            and .head.repo.full_name == $repository
            and (.head.sha | strings | test("^[a-f0-9]{40}$"))
            and (.number | type == "number" and . > 0 and floor == .)
        )) | {number, head: .head.sha}
    ' 2>/dev/null) || fail 'Source must be a merged repository PR targeting main.'
head=$(jq -er '.head' <<< "$pull")

ci=$(github "actions/workflows/ci.yml/runs?event=pull_request&head_sha=$head&per_page=100" |
    jq -ce --argjson pull "$pull" '
        ("FleetIQ PR \($pull.number) from \($pull.head) to main at ") as $prefix
        | .workflow_runs
        | map(select(.head_sha == $pull.head and .event == "pull_request"))
        | max_by([.id, .run_attempt])
        | select(.status == "completed" and .conclusion == "success" and .path == ".github/workflows/ci.yml")
        | . as $run
        | (.display_title | strings | select(startswith($prefix)) | ltrimstr($prefix)) as $tested
        | select($tested | test("^[a-f0-9]{40}$"))
        | {pull_request: $pull.number, ci_run: $run.id, tested_head: $pull.head, tested_revision: $tested}
    ' 2>/dev/null) || fail 'The latest PR CI run must have passed.'
tested=$(jq -er '.tested_revision' <<< "$ci")

github "git/commits/$tested" | jq -e --arg tested "$tested" --arg tree "$tree" '
    .sha == $tested and .tree.sha == $tree
' >/dev/null 2>&1 || fail 'Merged source tree differs from the exact passing CI checkout.'

jq -n --arg repository "$repository" --arg revision "$revision" \
    --arg run "$GITHUB_RUN_ID" --arg attempt "$GITHUB_RUN_ATTEMPT" \
    --arg actor "$actor" --arg trigger "$trigger" --arg tree "$tree" \
    --argjson ci "$ci" --argjson actor_authorization "$actor_authorization" \
    --argjson triggering_authorization "$triggering_authorization" '
    {
        format: "fleetiq-image-authorization.v1",
        event: "workflow_dispatch", repository: $repository, ref: "refs/heads/main",
        workflowRevision: $revision, revision: $revision, run: $run, attempt: $attempt,
        actor: $actor, triggeringActor: $trigger
    } + $ci + {tested_tree: $tree, actor_authorization: $actor_authorization,
        triggering_authorization: $triggering_authorization}
'

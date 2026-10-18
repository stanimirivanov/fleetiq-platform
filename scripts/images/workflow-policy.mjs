/** Exact workflow structure with independently updatable major action tags. */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  ensure,
  validateTools,
  validateDockerfile,
  readJson,
} from "./policy.mjs";
const actions = {
  checkout: "actions/checkout",
  "setup-node": "actions/setup-node",
  login: "docker/login-action",
  "upload-artifact": "actions/upload-artifact",
};
export function renderPublication(template, workflow) {
  const refs = new Map();
  for (const line of workflow.split("\n")) {
    const match = line.match(/^        uses: ([^@]+)@(.*)$/);
    if (!match) continue;
    ensure(
      Object.values(actions).includes(match[1]) &&
        /^v[1-9][0-9]*$/.test(match[2]),
      "Publication requires approved major-version action refs.",
    );
    ensure(
      !refs.has(match[1]) || refs.get(match[1]) === match[2],
      "Publication action refs are inconsistent.",
    );
    refs.set(match[1], match[2]);
  }
  for (const [slot, repository] of Object.entries(actions)) {
    ensure(
      template.includes("@action:" + slot + "@") && refs.has(repository),
      "Publication action slot is missing.",
    );
    template = template.replaceAll(
      "@action:" + slot + "@",
      refs.get(repository),
    );
  }
  ensure(!template.includes("@action:"), "Unknown publication action slot.");
  return template;
}
export function checkWorkflow(template, workflow) {
  const normalize = (text) => {
    ensure(
      !text.replaceAll("\r\n", "\n").includes("\r"),
      "Bare CR is unsupported.",
    );
    return text.replaceAll("\r\n", "\n");
  };
  workflow = normalize(workflow);
  template = normalize(template);
  ensure(
    renderPublication(template, workflow) === workflow,
    "Publication workflow differs from its reviewed template; review policy before regeneration.",
  );
}
export function validateContext(text) {
  const rules = text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#"));
  const required = [
    "*",
    "!Cargo.toml",
    "!Cargo.lock",
    "!crates/",
    "!crates/**",
    "!migrations/",
    "!migrations/**",
    "!.sqlx/",
    "!.sqlx/**",
    "!Dockerfile",
    "**/target",
    "**/.env",
    "**/.env.*",
    "**/hs_err_pid*.log",
    "**/replay_pid*.log",
    "**/.git",
  ];
  ensure(
    JSON.stringify(rules) === JSON.stringify(required),
    "Docker build context differs from the reviewed source allowlist.",
  );
}
export function checkProducer(root = process.cwd()) {
  validateTools(readJson(join(root, "scripts/images/tools.json")));
  const text = (path) =>
    readFileSync(join(root, path), "utf8").replaceAll("\r\n", "\n");
  validateDockerfile(
    text("Dockerfile"),
    text("rust-toolchain.toml").match(/channel = "([^"]+)"/)[1],
  );
  checkWorkflow(
    text(".github/publication.yml.template"),
    text(".github/workflows/publish-image.yml"),
  );
  validateContext(text(".dockerignore"));
  ensure(
    text("deploy/local/compose.yaml").includes('command: ["serve"]'),
    "Development Compose must explicitly select its compatibility mode.",
  );
}

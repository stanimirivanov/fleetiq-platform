/** Publish qualified bytes; registry credentials are never supplied to the build. */
import {
  readFileSync,
  writeFileSync,
  appendFileSync,
  lstatSync,
} from "node:fs";
import { resolve, join, relative } from "node:path";
import { homedir } from "node:os";
import { authorize } from "./authorization.mjs";
import { sourceIdentity } from "./source.mjs";
import { inspectLayout } from "./oci.mjs";
import { Resources } from "./resources.mjs";
import { cancellation } from "./process.mjs";
import { transport } from "./transport.mjs";
import { validateInventory } from "./inventory.mjs";
import {
  ensure,
  readJson,
  validateQualification,
  validateScan,
  hash,
  IMAGE,
} from "./policy.mjs";
export function validateEvidence(
  directory,
  source,
  now = Date.now(),
  sourceRoot = process.cwd(),
) {
  const reports = join(directory, "reports");
  const record = readJson(join(reports, "qualification.json"));
  validateQualification(record, source.revision);
  ensure(
    !source.dirty && JSON.stringify(record.source) === JSON.stringify(source),
    "Only the identical clean qualified source can publish.",
  );
  const artifact = inspectLayout(join(directory, "layout"), source.revision);
  ensure(
    record.image.digest === artifact.digest &&
      record.image.config === artifact.config &&
      record.image.manifest === artifact.manifest,
    "Qualification names another OCI artifact.",
  );
  for (const [name, expected] of [
    ["migrations", record.inventory.digest],
    ["scan", record.scan.digest],
    ["sbom", record.sbom.digest],
    ["provenance", record.provenance.digest],
  ])
    ensure(
      "sha256:" + hash(readFileSync(join(reports, name + ".json"))) ===
        expected,
      "Evidence bytes differ from their qualification receipt.",
    );
  ensure(
    "sha256:" + hash(artifact.sbom.bytes) === record.sbom.digest &&
      "sha256:" + hash(artifact.provenance.bytes) === record.provenance.digest,
    "Detached attestations differ from the actual image.",
  );
  validateScan(
    readJson(join(reports, "scan.json")),
    readJson(join(reports, "scanner.json")),
    now,
    artifact.config,
  );
  validateInventory(readJson(join(reports, "migrations.json")), sourceRoot);
  return { record, artifact, reports };
}
async function registryToken() {
  ensure(
    process.env.GITHUB_TOKEN && process.env.GITHUB_ACTOR,
    "Registry credential is unavailable.",
  );
  const response = await fetch(
    "https://ghcr.io/token?service=ghcr.io&scope=repository:stanimirivanov/fleetiq-platform:pull,push",
    {
      headers: {
        Authorization:
          "Basic " +
          Buffer.from(
            process.env.GITHUB_ACTOR + ":" + process.env.GITHUB_TOKEN,
          ).toString("base64"),
      },
      redirect: "error",
      signal: AbortSignal.timeout(15_000),
    },
  );
  ensure(
    response.ok,
    "Registry authorization failed (HTTP " + response.status + ").",
  );
  const { token } = await response.json();
  ensure(
    typeof token === "string" && token.length > 0,
    "Registry authorization returned no token.",
  );
  return token;
}
async function manifest(method, reference, token) {
  return await fetch(
    "https://ghcr.io/v2/stanimirivanov/fleetiq-platform/manifests/" + reference,
    {
      method,
      headers: {
        Authorization: "Bearer " + token,
        Accept: "application/vnd.oci.image.index.v1+json",
      },
      redirect: "error",
      signal: AbortSignal.timeout(15_000),
    },
  );
}
export async function publish() {
  const authorization = await authorize();
  const directory = resolve(process.env.FLEETIQ_QUALIFICATION_DIR ?? "");
  const child = relative(resolve("target/images"), directory);
  ensure(
    child.startsWith("run-") &&
      !child.includes("..") &&
      !child.includes("/") &&
      !child.includes("\\"),
    "Invalid qualification directory.",
  );
  ensure(
    lstatSync(directory).isDirectory() &&
      !lstatSync(directory).isSymbolicLink(),
    "Qualification directory must not redirect.",
  );
  const source = await sourceIdentity();
  ensure(
    source.revision === authorization.revision && !source.dirty,
    "Checked-out source differs from publication authorization.",
  );
  const { record, artifact, reports } = validateEvidence(directory, source);
  const tools = readJson("scripts/images/tools.json");
  const tag =
    "build-" +
    source.revision +
    "-" +
    authorization.run +
    "-" +
    authorization.attempt;
  const token = await registryToken();
  const previous = await manifest("HEAD", tag, token);
  ensure(
    previous.status === 404,
    "Publication reference exists or cannot be proved absent (HTTP " +
      previous.status +
      ").",
  );
  const config = resolve(
    process.env.DOCKER_CONFIG ?? join(homedir(), ".docker"),
    "config.json",
  );
  ensure(
    lstatSync(config).isFile(),
    "Registry login configuration is unavailable.",
  );
  const resources = new Resources();
  const cancel = cancellation();
  try {
    cancel.check();
    await transport(
      tools,
      directory,
      [
        "copy",
        "--all",
        "--preserve-digests",
        "--authfile",
        "/auth/config.json",
        "oci:/artifact/layout",
        "docker://" + IMAGE + ":" + tag,
      ],
      ["--volume", config + ":/auth/config.json:ro"],
      true,
    );
    const verified = await manifest("GET", artifact.digest, token);
    ensure(
      verified.ok &&
        verified.headers.get("docker-content-digest") === artifact.digest,
      "Published index is unavailable or has another digest.",
    );
    ensure(
      "sha256:" + hash(Buffer.from(await verified.arrayBuffer())) ===
        artifact.digest,
      "Published registry bytes differ.",
    );
    ensure(
      JSON.stringify(await sourceIdentity()) === JSON.stringify(source),
      "Source changed during publication.",
    );
    cancel.check();
  } finally {
    try {
      await resources.cleanup();
    } finally {
      cancel.dispose();
    }
  }
  const publication = {
    format: "fleetiq-image-publication.v1",
    outcome: "published",
    image: IMAGE + "@" + artifact.digest,
    discovery_tag: tag,
    source,
    platform: record.platform,
    inventory: record.inventory,
    provenance: record.provenance,
    sbom: record.sbom,
    qualification:
      "sha256:" + hash(readFileSync(join(reports, "qualification.json"))),
    scan: record.scan,
    authorization,
  };
  writeFileSync(
    join(reports, "publication.json"),
    JSON.stringify(publication, null, 2) + "\n",
  );
  if (process.env.GITHUB_STEP_SUMMARY)
    appendFileSync(
      process.env.GITHUB_STEP_SUMMARY,
      "Published backend: `" +
        publication.image +
        "`\n\nSource: `" +
        source.revision +
        "` (linux/amd64). This does not promote an environment.\n",
    );
  console.log("Published and verified: " + publication.image);
}

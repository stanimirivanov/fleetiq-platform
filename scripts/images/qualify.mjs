/** Build once, inspect, scan and execute the image; report success only after owned cleanup. */
import {
  mkdirSync,
  writeFileSync,
  readFileSync,
  appendFileSync,
  rmSync,
} from "node:fs";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";
import { docker, jsonDocker, cancellation } from "./process.mjs";
import { Resources } from "./resources.mjs";
import { inspectLayout } from "./oci.mjs";
import { sourceIdentity, localBuildCache } from "./source.mjs";
import {
  readJson,
  ensure,
  validateTools,
  validateDockerfile,
  hash,
  CHECKS,
  IMAGE,
  SBOM_CATALOGERS,
} from "./policy.mjs";
import { loadRuntime, roundtrip } from "./transport.mjs";
import { qualifyCompose } from "./compose.mjs";
import { qualifyRuntime } from "./runtime.mjs";
import { scanRuntime } from "./scan.mjs";
export async function qualify() {
  ensure(
    process.versions.node.split(".")[0] === "24",
    "Image tooling requires reviewed Node.js 24.",
  );
  const root = process.cwd();
  const tools = readJson("scripts/images/tools.json");
  validateTools(tools);
  validateDockerfile(
    readFileSync("Dockerfile", "utf8").replaceAll("\r\n", "\n"),
    readFileSync("rust-toolchain.toml", "utf8").match(/channel = "([^"]+)"/)[1],
  );
  const cache = localBuildCache(root, process.env.FLEETIQ_IMAGE_BUILD_CACHE);
  const resources = new Resources();
  const source = await sourceIdentity(root);
  const directory = resolve("target/images/run-" + resources.id);
  const reports = join(directory, "reports");
  mkdirSync(reports, { recursive: true });
  const record = {
    format: "fleetiq-image-qualification.v1",
    outcome: "incomplete",
    source,
    platform: tools.platform,
    checks: [],
  };
  const save = () =>
    writeFileSync(
      join(reports, "qualification.json"),
      JSON.stringify(record, null, 2) + "\n",
    );
  save();
  const cancel = cancellation();
  let builder = false;
  let loaded = false;
  let successful = false;
  let stage = "build";
  const tag = "fleetiq-qualified:" + resources.id;
  try {
    const builderName = resources.network;
    await docker([
      "buildx",
      "create",
      "--name",
      builderName,
      "--driver",
      "docker-container",
      "--driver-opt",
      "image=" + tools.buildkit,
    ]);
    builder = true;
    await docker(
      [
        "buildx",
        "build",
        "--builder",
        builderName,
        "--platform",
        tools.platform,
        "--build-arg",
        "SOURCE_REVISION=" + source.revision,
        "--tag",
        IMAGE + ":qualify-" + resources.id,
        "--provenance",
        "mode=max,version=v1",
        "--sbom",
        "generator=" +
          tools.sbom +
          ',"SELECT_CATALOGERS=' +
          SBOM_CATALOGERS +
          '"',
        ...cache,
        ...(cache.length
          ? []
          : [
              "--cache-to",
              "type=local,dest=" + join(directory, "build-cache") + ",mode=max",
            ]),
        "--metadata-file",
        join(directory, "build.json"),
        "--output",
        "type=oci,dest=" + join(directory, "layout") + ",tar=false",
        root,
      ],
      {
        capture: false,
        timeout: 3_600_000,
        env: {
          ...process.env,
          BUILDX_GIT_INFO: "true",
          BUILDX_GIT_CHECK_DIRTY: "true",
        },
      },
    );
    cancel.check();
    stage = "oci-metadata";
    const artifact = inspectLayout(join(directory, "layout"), source.revision);
    ensure(
      readJson(join(directory, "build.json"))["containerimage.digest"] ===
        artifact.digest,
      "Build output digest differs.",
    );
    record.image = {
      digest: artifact.digest,
      manifest: artifact.manifest,
      config: artifact.config,
    };
    for (const name of ["sbom", "provenance"]) {
      writeFileSync(join(reports, name + ".json"), artifact[name].bytes);
      record[name] = { digest: "sha256:" + hash(artifact[name].bytes) };
    }
    stage = "runtime-export";
    await resources.start();
    await loadRuntime(tools, directory, tag, resources);
    loaded = true;
    stage = "scan";
    record.scan = await scanRuntime(
      directory,
      tools,
      artifact.config,
      resources,
    );
    stage = "runtime";
    const runtime = await qualifyRuntime(
      tag,
      tools,
      artifact,
      root,
      resources,
      directory,
    );
    const inventory = JSON.stringify(runtime.inventory, null, 2) + "\n";
    writeFileSync(join(reports, "migrations.json"), inventory);
    record.inventory = {
      digest: "sha256:" + hash(inventory),
      count: runtime.inventory.migrations.length,
    };
    record.checks = runtime.checks;
    stage = "development-compose";
    await qualifyCompose(tag, directory, resources, tools);
    record.checks.push("development-compose");
    stage = "registry-roundtrip";
    await roundtrip(tools, directory, artifact, resources);
    ensure(
      inspectLayout(join(directory, "roundtrip"), source.revision).digest ===
        artifact.digest,
      "Roundtrip did not preserve the complete OCI artifact.",
    );
    record.checks.push("registry-roundtrip");
    stage = "source-integrity";
    ensure(
      JSON.stringify(await sourceIdentity(root)) === JSON.stringify(source),
      "Build sources changed during qualification.",
    );
    cancel.check();
    successful = true;
  } finally {
    stage = successful ? "cleanup" : stage;
    try {
      try {
        await resources.cleanup();
      } finally {
        if (builder)
          await docker(["buildx", "rm", resources.network], {
            timeout: 120_000,
          });
      }
      if (loaded) {
        ensure(
          (await jsonDocker(["image", "inspect", tag]))[0].Id ===
            record.image.config,
          "Loaded image ownership changed.",
        );
        await docker(["image", "rm", tag]);
      }
      if (successful) {
        record.checks.push("cleanup");
        ensure(
          JSON.stringify(record.checks) === JSON.stringify(CHECKS),
          "Qualification checks differ.",
        );
        record.outcome = "passed";
      }
    } finally {
      record.stage = stage;
      save();
      cancel.dispose();
    }
  }
  rmSync(join(directory, "runtime.tar"));
  if (process.env.GITHUB_OUTPUT)
    appendFileSync(
      process.env.GITHUB_OUTPUT,
      "directory=" + directory.replaceAll("\\", "/") + "\n",
    );
  console.log(
    "Image qualification passed: " +
      record.image.digest +
      "; evidence: " +
      reports,
  );
  return directory;
}
if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href
) {
  try {
    await qualify();
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

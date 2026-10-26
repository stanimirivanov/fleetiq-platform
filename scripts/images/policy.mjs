/** Producer invariants; these validate metadata, not release approval or signatures. */
import { createHash } from "node:crypto";
import { readFileSync, lstatSync } from "node:fs";
export const SOURCE = "https://github.com/stanimirivanov/fleetiq-platform";
export const REPOSITORY = "stanimirivanov/fleetiq-platform";
export const IMAGE = "ghcr.io/stanimirivanov/fleetiq-platform";
export const SBOM_CATALOGERS =
  "image,file,+sbom-cataloger,+rust-cargo-lock-cataloger";
export const SHA = /^[a-f0-9]{40}$/;
export const DIGEST = /^sha256:[a-f0-9]{64}$/;
export const CHECKS = [
  "runtime-config",
  "empty-schema-refused",
  "explicit-migration",
  "health-and-discovery",
  "read-only-nonroot",
  "graceful-stop",
  "inventory",
  "development-compose",
  "registry-roundtrip",
  "cleanup",
];
export const hash = (bytes, algorithm = "sha256") =>
  createHash(algorithm).update(bytes).digest("hex");
export const ensure = (condition, reason) => {
  if (!condition) throw new Error(reason);
};
export function readJson(path) {
  const meta = lstatSync(path);
  ensure(
    meta.isFile() && !meta.isSymbolicLink() && meta.size < 32 * 1024 * 1024,
    "Expected bounded regular evidence file.",
  );
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    throw new Error("Malformed image evidence JSON.");
  }
}
export function validateTools(tools) {
  ensure(
    tools.format === "fleetiq-image-tools.v1" &&
      tools.platform === "linux/amd64",
    "Unsupported image tool contract.",
  );
  ensure(
    /^quay\.io\/containers\/skopeo:v\d+\.\d+\.\d+-immutable@sha256:[a-f0-9]{64}$/u.test(tools.transport),
    "Transport requires the upstream immutable channel and a reviewed digest.",
  );
  const repositories = {
    buildkit: "moby/buildkit",
    sbom: "docker/buildkit-syft-scanner",
    transport: "quay.io/containers/skopeo",
    scanner: "aquasec/trivy",
    postgres: "postgres",
    registry: "registry",
  };
  for (const [name, repository] of Object.entries(repositories)) {
    const ref = tools[name];
    const [image, digest] = ref.split("@");
    ensure(
      image.split(":")[0] === repository && DIGEST.test(digest),
      "Every tool requires its reviewed repository and immutable digest.",
    );
  }
}
export function validateDockerfile(text, rust) {
  const from = [...text.matchAll(/^FROM ([^ ]+) AS (builder|runtime)$/gm)];
  ensure(
    from.length === 2 && from.every((x) => /@sha256:[a-f0-9]{64}$/.test(x[1])),
    "Both base images require immutable digests.",
  );
  ensure(
    from[0][1].startsWith("rust:" + rust + "-slim-"),
    "Container Rust must match rust-toolchain.toml.",
  );
  ensure(
    from[1][1].startsWith("gcr.io/distroless/cc-debian13:nonroot@"),
    "Runtime must use the reviewed shell-free Debian family.",
  );
  ensure(
    text.includes('CMD ["/usr/local/bin/fleetiq-server", "healthcheck"]'),
    "Container readiness must use the bounded application probe.",
  );
  ensure(/^USER 10001:10001$/m.test(text), "Runtime must be nonroot.");
  ensure(
    text.includes("cp target/release/fleetiq-protocol-live /tmp/fleetiq-protocol-live") &&
      text.includes("COPY --from=builder /tmp/fleetiq-protocol-live /usr/local/bin/fleetiq-protocol-live"),
    "Production must package the fixed isolated live executor.",
  );
  ensure(
    text.includes("cp target/release/fleetiq-protocol-decode /tmp/fleetiq-protocol-decode") &&
      text.includes("COPY --from=builder /tmp/fleetiq-protocol-decode /usr/local/bin/fleetiq-protocol-decode"),
    "Production must package the fixed isolated replay executor.",
  );
  ensure(
    text.includes("-p fleetiq-server -p fleetiq-protocol-wasm --bins") &&
      text.includes("cp target/release/fleetiq-protocol-admission /tmp/fleetiq-protocol-admission") &&
      text.includes("COPY --from=builder /tmp/fleetiq-protocol-admission /usr/local/bin/fleetiq-protocol-admission"),
    "Production must build and package the reviewed isolated admission worker.",
  );
  ensure(
    /^CMD \["serve", "--check-schema"\]$/m.test(text),
    "Production must default to checked serving.",
  );
  ensure(
    text.includes(
      "SQLX_OFFLINE=true cargo build --locked --release -p fleetiq-server",
    ),
    "Image build must use locked offline SQLx compilation.",
  );
}
export function validateRuntime(config, revision) {
  ensure(
    config.os === "linux" && config.architecture === "amd64",
    "Unsupported runtime platform.",
  );
  ensure(config.config.User === "10001:10001", "Image user differs.");
  ensure(
    JSON.stringify(config.config.Entrypoint) ===
      JSON.stringify(["/usr/local/bin/fleetiq-server"]),
    "Image entrypoint differs.",
  );
  ensure(
    JSON.stringify(config.config.Cmd) ===
      JSON.stringify(["serve", "--check-schema"]),
    "Image checked-mode default differs.",
  );
  ensure(
    config.config.Labels?.["org.opencontainers.image.revision"] === revision &&
      config.config.Labels?.["org.opencontainers.image.source"] === SOURCE,
    "Image source labels differ.",
  );
}
export function validateScan(scan, database, now = Date.now(), expectedConfig) {
  if (expectedConfig)
    ensure(
      scan.Metadata?.ImageID === expectedConfig,
      "Scan names a different runtime image.",
    );
  ensure(
    scan.SchemaVersion === 2 &&
      Array.isArray(scan.Results) &&
      scan.Results.length > 0,
    "Missing image scan results.",
  );
  ensure(
    scan.Metadata?.OS?.Family === "debian" &&
      scan.Results.some((r) => r.Class === "os-pkgs" && r.Packages?.length),
    "Runtime OS packages were not scanned.",
  );
  for (const result of scan.Results) {
    ensure(!result.Secrets?.length, "Secret finding in runtime image.");
    ensure(
      !(result.Vulnerabilities ?? []).some((v) =>
        ["HIGH", "CRITICAL"].includes(v.Severity),
      ),
      "High/critical runtime vulnerability.",
    );
  }
  ensure(
    database.Version === "0.76.0" && database.VulnerabilityDB?.Version === 2,
    "Unexpected vulnerability scanner/database.",
  );
  const updated = Date.parse(database.VulnerabilityDB.UpdatedAt);
  const next = Date.parse(database.VulnerabilityDB.NextUpdate);
  ensure(
    Number.isFinite(updated) &&
      now >= updated &&
      now - updated <= 86_400_000 &&
      next > now,
    "Vulnerability database is stale or unavailable.",
  );
}
export function validateQualification(record, revision) {
  ensure(
    record.format === "fleetiq-image-qualification.v1" &&
      record.outcome === "passed",
    "Image qualification is incomplete.",
  );
  ensure(
    record.source.revision === revision &&
      SHA.test(revision) &&
      record.source.repository === REPOSITORY,
    "Qualification source differs.",
  );
  ensure(
    record.platform === "linux/amd64" &&
      DIGEST.test(record.image.digest) &&
      DIGEST.test(record.image.config),
    "Invalid artifact identity.",
  );
  ensure(
    JSON.stringify(record.checks) === JSON.stringify(CHECKS) &&
      record.scan?.outcome === "passed",
    "Required artifact checks are missing.",
  );
  ensure(
    DIGEST.test(record.inventory?.digest) &&
      DIGEST.test(record.scan.digest) &&
      DIGEST.test(record.provenance?.digest) &&
      DIGEST.test(record.sbom?.digest),
    "Artifact evidence is not content bound.",
  );
}

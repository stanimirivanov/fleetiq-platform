/** OCI content checks supplement maintained BuildKit/Skopeo transports. */
import { readFileSync, lstatSync } from "node:fs";
import { join } from "node:path";
import { ensure, hash, DIGEST, readJson, validateRuntime } from "./policy.mjs";
function blob(layout, descriptor) {
  ensure(
    DIGEST.test(descriptor.digest) && Number.isSafeInteger(descriptor.size),
    "Invalid OCI descriptor.",
  );
  const path = join(layout, "blobs", "sha256", descriptor.digest.slice(7));
  const metadata = lstatSync(path);
  ensure(
    metadata.isFile() &&
      !metadata.isSymbolicLink() &&
      metadata.size === descriptor.size &&
      metadata.size <= 32 * 1024 * 1024,
    "Invalid OCI metadata blob.",
  );
  const bytes = readFileSync(path);
  ensure(
    "sha256:" + hash(bytes) === descriptor.digest,
    "OCI metadata digest differs.",
  );
  return { json: JSON.parse(bytes), bytes };
}
export function inspectLayout(layout, revision) {
  const index = readJson(join(layout, "index.json"));
  ensure(
    index.schemaVersion === 2 && index.manifests?.length === 1,
    "Expected exactly one exported OCI root.",
  );
  const descriptor = index.manifests[0];
  const root = blob(layout, descriptor).json;
  ensure(
    root.schemaVersion === 2 && root.manifests?.length === 2,
    "Expected one platform and one attestation manifest.",
  );
  const image = root.manifests.find(
    (m) => m.platform?.os === "linux" && m.platform?.architecture === "amd64",
  );
  const attestation = root.manifests.find(
    (m) =>
      m.annotations?.["vnd.docker.reference.type"] === "attestation-manifest",
  );
  ensure(
    image &&
      attestation &&
      attestation.annotations["vnd.docker.reference.digest"] === image.digest,
    "Attestations do not name this runtime image.",
  );
  const manifest = blob(layout, image).json;
  const config = blob(layout, manifest.config).json;
  validateRuntime(config, revision);
  const statements = blob(layout, attestation).json.layers.map((layer) =>
    blob(layout, layer),
  );
  for (const statement of statements)
    ensure(
      statement.json.subject?.some(
        (s) => s.digest?.sha256 === image.digest.slice(7),
      ),
      "Attestation subject differs.",
    );
  const sboms = statements.filter(
    (s) => s.json.predicateType === "https://spdx.dev/Document",
  );
  const sbom = sboms.find((s) => s.json.predicate?.name === "sbom");
  const hasPackage = (s, prefix) =>
    s?.json.predicate?.packages?.some((p) =>
      p.externalRefs?.some((ref) => ref.referenceLocator?.startsWith(prefix)),
    );
  ensure(
    hasPackage(sbom, "pkg:deb/debian/"),
    "Missing runtime Debian SBOM coverage.",
  );
  ensure(
    sboms.some(
      (s) =>
        s.json.predicate?.name === "sbom-builder" &&
        hasPackage(s, "pkg:cargo/"),
    ),
    "Missing declared Rust lockfile packages in the builder SBOM.",
  );
  const provenance = statements.find(
    (s) => s.json.predicateType === "https://slsa.dev/provenance/v1",
  );
  ensure(
    sbom?.json.predicate?.packages?.length > 0 && provenance,
    "Missing SPDX SBOM or supported build provenance.",
  );
  // These are unsigned BuildKit statements, not a trusted-builder signature.
  const vcs =
    provenance.json.predicate.runDetails?.metadata?.buildkit_metadata?.vcs;
  ensure(
    vcs?.revision?.replace("-dirty", "") === revision,
    "Build provenance source revision differs.",
  );
  ensure(
    [
      "https://github.com/stanimirivanov/fleetiq-platform",
      "https://github.com/stanimirivanov/fleetiq-platform.git",
      "git@github.com:stanimirivanov/fleetiq-platform.git",
    ].includes(vcs.source),
    "Build provenance repository differs.",
  );
  return {
    digest: descriptor.digest,
    manifest: image.digest,
    runtime: image,
    config: manifest.config.digest,
    provenance,
    sbom,
  };
}

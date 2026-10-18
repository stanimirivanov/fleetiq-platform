import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, sep } from "node:path";
import { hash } from "../policy.mjs";
export function layoutFixture() {
  const directory = mkdtempSync(join(tmpdir(), "fleetiq-image-oci-"));
  const root = resolve(directory);
  const parent = resolve(tmpdir());
  if (!root.startsWith(parent + sep))
    throw new Error("Fixture escaped its temporary parent.");
  const layout = join(root, "layout");
  mkdirSync(join(layout, "blobs", "sha256"), { recursive: true });
  const revision = "a".repeat(40);
  const blob = (object) => {
    const bytes = Buffer.from(JSON.stringify(object));
    const digest = "sha256:" + hash(bytes);
    writeFileSync(join(layout, "blobs", "sha256", digest.slice(7)), bytes);
    return {
      mediaType: "application/vnd.oci.image.manifest.v1+json",
      digest,
      size: bytes.length,
    };
  };
  const config = blob({
    os: "linux",
    architecture: "amd64",
    config: {
      User: "10001:10001",
      Entrypoint: ["/usr/local/bin/fleetiq-server"],
      Cmd: ["serve", "--check-schema"],
      Labels: {
        "org.opencontainers.image.revision": revision,
        "org.opencontainers.image.source":
          "https://github.com/stanimirivanov/fleetiq-platform",
      },
    },
  });
  const image = {
    ...blob({ schemaVersion: 2, config, layers: [] }),
    platform: { os: "linux", architecture: "amd64" },
  };
  const statement = (predicateType, predicate) => ({
    _type: "https://in-toto.io/Statement/v0.1",
    subject: [{ name: "image", digest: { sha256: image.digest.slice(7) } }],
    predicateType,
    predicate,
  });
  const sbom = blob(
    statement("https://spdx.dev/Document", {
      name: "sbom",
      packages: [
        {
          name: "libc6",
          externalRefs: [
            {
              referenceType: "purl",
              referenceLocator: "pkg:deb/debian/libc6@2.41",
            },
          ],
        },
      ],
    }),
  );
  const buildSbom = blob(
    statement("https://spdx.dev/Document", {
      name: "sbom-builder",
      packages: [
        {
          name: "serde",
          externalRefs: [
            {
              referenceType: "purl",
              referenceLocator: "pkg:cargo/serde@1.0.229",
            },
          ],
        },
      ],
    }),
  );
  const provenance = blob(
    statement("https://slsa.dev/provenance/v1", {
      runDetails: {
        metadata: {
          buildkit_metadata: {
            vcs: {
              revision,
              source: "https://github.com/stanimirivanov/fleetiq-platform.git",
            },
          },
        },
      },
    }),
  );
  const attestation = {
    ...blob({ schemaVersion: 2, layers: [sbom, buildSbom, provenance] }),
    platform: { os: "unknown", architecture: "unknown" },
    annotations: {
      "vnd.docker.reference.type": "attestation-manifest",
      "vnd.docker.reference.digest": image.digest,
    },
  };
  const index = blob({ schemaVersion: 2, manifests: [image, attestation] });
  writeFileSync(
    join(layout, "index.json"),
    JSON.stringify({ schemaVersion: 2, manifests: [index] }),
  );
  return {
    directory,
    layout,
    revision,
    index,
    image,
    config,
    sbom,
    buildSbom,
    provenance,
    attestation,
    blob,
    cleanup: () => rmSync(root, { recursive: true }),
  };
}

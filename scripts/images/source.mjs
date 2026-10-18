/** Bind a candidate to Git and every reviewable source byte, including its harness. */
import { readFileSync, lstatSync, realpathSync } from "node:fs";
import { join, resolve, relative, isAbsolute, sep } from "node:path";
import { hash, ensure, SHA, REPOSITORY } from "./policy.mjs";
import { run } from "./process.mjs";
export async function sourceIdentity(root = process.cwd()) {
  const origin = (await run("git", ["remote", "get-url", "origin"])).trim();
  ensure(
    [
      "https://github.com/" + REPOSITORY,
      "https://github.com/" + REPOSITORY + ".git",
      "git@github.com:" + REPOSITORY + ".git",
    ].includes(origin),
    "Build origin must be the canonical credential-free repository URL.",
  );
  const revision = (await run("git", ["rev-parse", "HEAD"])).trim();
  ensure(SHA.test(revision), "Expected full source revision.");
  const dirty =
    (
      await run("git", ["status", "--porcelain=v1", "--untracked-files=normal"])
    ).trim().length > 0;
  const paths = (
    await run("git", [
      "ls-files",
      "--cached",
      "--others",
      "--exclude-standard",
      "-z",
    ])
  )
    .split("\0")
    .filter(Boolean)
    .sort();
  const files = paths.map((path) => {
    let metadata;
    try {
      metadata = lstatSync(join(root, path));
    } catch (error) {
      if (error.code === "ENOENT") return [path, "deleted"];
      throw error;
    }
    ensure(
      metadata.isFile() &&
        !metadata.isSymbolicLink() &&
        metadata.size <= 128 * 1024 * 1024,
      "Reviewable source must contain bounded regular files.",
    );
    return [path, hash(readFileSync(join(root, path)))];
  });
  return {
    repository: REPOSITORY,
    revision,
    dirty,
    fingerprint: "sha256:" + hash(JSON.stringify(files)),
  };
}

/** Explicit local optimization only; every output still passes full qualification. */
export function localBuildCache(root, candidate) {
  if (!candidate) return [];
  const target = realpathSync(join(root, "target"));
  const path = resolve(root, candidate);
  const rel = relative(target, path);
  ensure(
    rel && !rel.startsWith("..") && !isAbsolute(rel),
    "Build cache must be inside this checkout's ignored target directory.",
  );
  let current = target;
  for (const part of rel.split(sep)) {
    current = join(current, part);
    const entry = lstatSync(current);
    ensure(
      entry.isDirectory() && !entry.isSymbolicLink(),
      "Build cache must not traverse links.",
    );
  }
  ensure(
    realpathSync(path) === path,
    "Build cache resolves outside its selected path.",
  );
  ensure(
    readJsonCache(join(path, "index.json")).schemaVersion === 2,
    "Build cache must have an OCI index.",
  );
  ensure(
    !path.includes(","),
    "Build cache path must not contain an option delimiter.",
  );
  return ["--cache-from", "type=local,src=" + path];
}
function readJsonCache(path) {
  const entry = lstatSync(path);
  ensure(
    entry.isFile() && !entry.isSymbolicLink() && entry.size < 1024 * 1024,
    "Build cache index must be a bounded regular file.",
  );
  return JSON.parse(readFileSync(path, "utf8"));
}

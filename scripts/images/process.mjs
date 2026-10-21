/** Bounded subprocesses: callers provide argument vectors, never shell text. */
import { openSync, closeSync, fstatSync } from "node:fs";
import { randomUUID } from "node:crypto";
import { spawn } from "node:child_process";
import { diagnosticCategory, ImageToolFailure } from "./failure.mjs";
const children = new Set();
export function cancellation() {
  let interrupted = false;
  const stop = () => {
    interrupted = true;
    for (const child of children) child.kill();
  };
  globalThis.process.on("SIGINT", stop);
  globalThis.process.on("SIGTERM", stop);
  return {
    check: () => {
      if (interrupted) throw new Error("Image operation was interrupted.");
    },
    dispose: () => {
      globalThis.process.off("SIGINT", stop);
      globalThis.process.off("SIGTERM", stop);
    },
  };
}
export async function run(
  program,
  args,
  {
    capture = true,
    timeout = 120_000,
    env = process.env,
    output,
    maxOutputBytes = 512 * 1024 * 1024,
  } = {},
) {
  if (
    output &&
    (!Number.isSafeInteger(maxOutputBytes) ||
      maxOutputBytes <= 0 ||
      maxOutputBytes > 512 * 1024 * 1024)
  )
    throw new Error("Invalid binary output bound.");
  // A native descriptor preserves arbitrary bytes and avoids a JS/string copy.
  // Exclusive creation rejects links and accidental replacement of prior output.
  const descriptor = output ? openSync(output, "wx", 0o600) : undefined;
  try {
    return await new Promise((resolve, reject) => {
      const child = spawn(program, args, {
        shell: false,
        windowsHide: true,
        env,
        stdio: [
          "ignore",
          descriptor ?? (capture ? "pipe" : "inherit"),
          capture ? "pipe" : "inherit",
        ],
      });
      children.add(child);
      child.stdout?.setEncoding("utf8");
      child.stderr?.setEncoding("utf8");
      let output = "";
      let bytes = 0;
      let diagnosticTail = "";
      let category = "command-failed";
      let exceeded = false;
      let settled = false;
      const append = (target, chunk) => {
        bytes += Buffer.byteLength(chunk);
        if (bytes > 32 * 1024 * 1024) {
          exceeded = true;
          child.kill();
          return;
        }
        if (target === "out") output += chunk;
        else {
          // Keep a bounded private window only to recognize split Docker errors.
          // Neither this text nor argv is attached to errors or qualification reports.
          diagnosticTail = (diagnosticTail + chunk).slice(-4096);
          const observed = diagnosticCategory(diagnosticTail);
          if (observed !== "command-failed") category = observed;
        }
      };
      child.stdout?.on("data", (c) => append("out", c));
      child.stderr?.on("data", (c) => append("err", c));
      const timer = setTimeout(() => {
        exceeded = true;
        child.kill();
      }, timeout);
      const monitor =
        descriptor === undefined
          ? undefined
          : setInterval(() => {
              if (fstatSync(descriptor).size > maxOutputBytes) {
                exceeded = true;
                child.kill();
              }
            }, 100);
      child.on("error", (error) => {
        if (settled) return;
        settled = true;
        children.delete(child);
        clearTimeout(timer);
        clearInterval(monitor);
        reject(
          new ImageToolFailure(
            args[0], null, false,
            error.code === "ENOENT" ? "tool-unavailable" : "tool-start-failed",
          ),
        );
      });
      child.on("close", (code) => {
        if (settled) return;
        settled = true;
        children.delete(child);
        clearTimeout(timer);
        clearInterval(monitor);
        if (
          descriptor !== undefined &&
          fstatSync(descriptor).size > maxOutputBytes
        )
          exceeded = true;
        if (code !== 0 || exceeded)
          reject(
            new ImageToolFailure(args[0], code, exceeded, category),
          );
        else resolve(output);
      });
    });
  } finally {
    if (descriptor !== undefined) closeSync(descriptor);
  }
}
// Match bind-mounted file ownership on Linux; Docker Desktop maps Windows ACLs.
export function containerUser(
  uid = process.getuid?.() ?? 0,
  gid = process.getgid?.() ?? 0,
) {
  if (![uid, gid].every((id) => Number.isSafeInteger(id) && id >= 0))
    throw new Error("Invalid container filesystem identity.");
  return uid + ":" + gid;
}
let owner;
export function setDockerOwner(value) {
  owner = value;
}
export function docker(args, options) {
  if (args[0] === "run" && owner) {
    const explicit = args.indexOf("--name");
    const name =
      explicit >= 0
        ? args[explicit + 1]
        : owner.network + "-tool-" + randomUUID();
    if (!owner.containers.includes(name)) owner.containers.push(name);
    args = [
      "run",
      ...(explicit >= 0 ? [] : ["--name", name]),
      "--label",
      owner.label + "=" + owner.id,
      ...args.slice(1),
    ];
  }
  return run(process.env.FLEETIQ_DOCKER ?? "docker", args, options);
}
export const jsonDocker = async (args) => JSON.parse(await docker(args));

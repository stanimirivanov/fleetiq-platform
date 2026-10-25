/** Classify tool failures without retaining command arguments or child output. */
const categories = [
  [
    "storage-exhausted",
    /no space left on device|disk quota exceeded|not enough space on (?:the )?disk/iu,
  ],
  [
    "daemon-unavailable",
    /cannot connect to (?:the )?docker daemon|is the docker daemon running|error during connect/iu,
  ],
  [
    "registry-authentication",
    /pull access denied|authentication required|unauthorized:|denied: requested access/iu,
  ],
  [
    "network-or-timeout",
    /i\/o timeout|TLS handshake timeout|context deadline exceeded|no such host|network is unreachable|connection refused/iu,
  ],
  [
    "image-unavailable",
    /manifest unknown|no such manifest|no matching manifest|failed to resolve reference/iu,
  ],
  [
    "invalid-mount",
    /invalid mount config|invalid volume specification|bind source path does not exist|mounts denied/iu,
  ],
  [
    "entrypoint-unavailable",
    /executable file not found|exec:.*no such file or directory/iu,
  ],
  [
    "permission-denied",
    /permission denied|operation not permitted/iu,
  ],
];
const operations = new Set([
  "run", "exec", "load", "image", "pull", "buildx", "inspect", "network",
  "ps", "rm", "compose", "version", "info", "stop", "wait", "logs", "port",
]);

export function diagnosticCategory(stderr) {
  return (
    categories.find(([, pattern]) => pattern.test(stderr))?.[0] ?? "command-failed"
  );
}

export class ImageToolFailure extends Error {
  constructor(operation, code, bounded, category) {
    const details = Object.freeze({
      operation: operations.has(operation) ? operation : "tool",
      exitCode: Number.isInteger(code) ? code : null,
      bounded,
      category,
    });
    super(
      `Image tool ${details.operation} failed (exit ${details.exitCode}, bounded=${bounded}, category=${category}).`,
    );
    this.name = "ImageToolFailure";
    this.details = details;
  }
}

/** Add only a fixed prerequisite name; never propagate child output or argv. */
export class ImagePrerequisiteFailure extends Error {
  constructor(phase, cause) {
    const prerequisite = new Set([
      "transport-remote-manifest", "transport-manifest-validation", "transport-sandbox",
    ]).has(phase) ? phase : "unknown";
    const details = Object.freeze({ ...failureDetails(cause), prerequisite });
    super(`Image prerequisite ${prerequisite} failed (category=${details.category}); check the reviewed transport pin and registry availability.`);
    this.name = "ImagePrerequisiteFailure";
    this.details = details;
  }
}

/** Unknown errors may quote runtime configuration, so retain no message or stack. */
export function failureDetails(error) {
  return error instanceof ImageToolFailure || error instanceof ImagePrerequisiteFailure
    ? error.details
    : { category: "operation-failed" };
}

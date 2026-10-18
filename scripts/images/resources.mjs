/** Every transient Docker resource has a unique owner label and checked cleanup. */
import { randomUUID } from "node:crypto";
import { docker, jsonDocker, setDockerOwner } from "./process.mjs";
import { ensure } from "./policy.mjs";
export class Resources {
  id = randomUUID();
  containers = [];
  network = "fiq-image-" + this.id;
  label = "io.fleetiq.image-qualification";
  constructor() {
    setDockerOwner(this);
  }
  async start() {
    await docker([
      "network",
      "create",
      "--label",
      this.label + "=" + this.id,
      this.network,
    ]);
  }
  async container(suffix, args, network = this.network) {
    const name = this.network + "-" + suffix;
    this.containers.push(name);
    await docker([
      "run",
      "--detach",
      "--name",
      name,
      "--label",
      this.label + "=" + this.id,
      "--network",
      network,
      ...args,
    ]);
    return name;
  }
  async cleanup() {
    const owned = (
      await docker([
        "ps",
        "--all",
        "--filter",
        "label=" + this.label + "=" + this.id,
        "--format",
        "{{.Names}}",
      ])
    )
      .trim()
      .split(/\r?\n/)
      .filter(Boolean);
    for (const name of owned) {
      ensure(
        this.containers.includes(name),
        "Unexpected resource appeared in the owned fixture.",
      );
      const [container] = await jsonDocker(["inspect", name]);
      ensure(
        container.Config.Labels[this.label] === this.id,
        "Container ownership changed.",
      );
      await docker(["rm", "--force", "--volumes", name]);
    }
    const networks = (
      await docker([
        "network",
        "ls",
        "--filter",
        "label=" + this.label + "=" + this.id,
        "--format",
        "{{.Name}}",
      ])
    )
      .trim()
      .split(/\r?\n/)
      .filter(Boolean);
    for (const network of networks) {
      ensure(
        network === this.network,
        "Unexpected network appeared in the owned fixture.",
      );
      const [metadata] = await jsonDocker(["network", "inspect", network]);
      ensure(
        metadata.Labels[this.label] === this.id,
        "Network ownership changed.",
      );
      await docker(["network", "rm", network]);
    }
    ensure(
      !(
        await docker([
          "ps",
          "--all",
          "--filter",
          "label=" + this.label + "=" + this.id,
          "--format",
          "{{.Names}}",
        ])
      ).trim(),
      "Owned containers remain.",
    );
  }
}
export async function until(probe, timeout = 60_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await probe()) return;
    // Polling observes an external process; this is a deadline, not test coordination.
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error("Image probe deadline expired.");
}

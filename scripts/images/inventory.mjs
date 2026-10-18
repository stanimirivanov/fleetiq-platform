/** Validate the binary's D01 inventory against source; do not reimplement migrations. */
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { ensure, hash } from "./policy.mjs";
export function validateInventory(inventory, root) {
  ensure(
    inventory.format === "fleetiq-migrations.v1" &&
      inventory.checksum_algorithm === "sha384",
    "Unsupported migration inventory.",
  );
  ensure(Array.isArray(inventory.migrations), "Missing migration entries.");
  const files = readdirSync(join(root, "migrations")).filter((f) =>
    /^\d+_.+\.sql$/.test(f),
  );
  const expected = new Map(
    files.map((name) => [
      BigInt(name.split("_")[0]).toString(),
      {
        checksum: hash(readFileSync(join(root, "migrations", name)), "sha384"),
        description: name.slice(name.indexOf("_") + 1, -4).replaceAll("_", " "),
      },
    ]),
  );
  ensure(
    expected.size === files.length &&
      inventory.migrations.length === expected.size,
    "Migration inventory is incomplete.",
  );
  let previous = 0n;
  for (const item of inventory.migrations) {
    ensure(
      typeof item.version === "string" && /^[1-9][0-9]*$/.test(item.version),
      "Migration versions must be exact decimal text.",
    );
    const version = BigInt(item.version);
    ensure(
      version > previous && version <= 9223372036854775807n,
      "Migration ordering/range differs.",
    );
    ensure(
      expected.get(item.version)?.checksum === item.checksum &&
        expected.get(item.version)?.description === item.description &&
        typeof item.description === "string" &&
        item.description.length > 0,
      "Migration checksum or description differs.",
    );
    previous = version;
  }
}

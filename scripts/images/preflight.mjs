import { mkdirSync, writeFileSync } from "node:fs";
import { authorize } from "./authorization.mjs";
try {
  const authorization = await authorize();
  mkdirSync("target/images", { recursive: true });
  writeFileSync(
    "target/images/authorization.json",
    JSON.stringify(authorization, null, 2) + "\n",
  );
  console.log(
    "Publication prerequisites passed for " +
      authorization.revision +
      ". Manual dispatch authorization has been verified.",
  );
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}

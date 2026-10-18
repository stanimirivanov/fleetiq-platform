import { publish } from "./publication.mjs";
try {
  await publish();
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}

import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

/** Compare bytes without changing the committed contract, including on failure. */
export function checkBundle({ source, config, committed, run, temporaryParent = tmpdir() }) {
  const temporaryDirectory = mkdtempSync(join(temporaryParent, 'fleetiq-openapi-'));
  const generated = join(temporaryDirectory, 'openapi.json');
  try {
    run([
      'bundle', source,
      '--config', config,
      '--output', generated,
      '--component-renaming-conflicts-severity=error',
    ]);
    if (!readFileSync(generated).equals(readFileSync(committed))) {
      throw new Error('OpenAPI bundle is stale; run pnpm bundle:openapi and commit contracts/http/dist/openapi.json.');
    }
  } finally {
    // Throwing instead of process.exit lets finally clean up failed launches,
    // nonzero exits, missing output, and stale artifacts as well as successful runs.
    rmSync(temporaryDirectory, { recursive: true, force: true });
  }
}

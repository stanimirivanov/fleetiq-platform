import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const repositoryRoot = fileURLToPath(new URL('../../', import.meta.url));

// Reuse the reviewed CLI distribution rather than importing its bundled internals
// or installing a second YAML parser with potentially different semantics.
export function redoclyCli() {
  const require = createRequire(import.meta.url);
  const packagePath = require.resolve('@redocly/cli/package.json');
  const installed = JSON.parse(readFileSync(packagePath, 'utf8'));
  const expected = JSON.parse(readFileSync(resolve(repositoryRoot, 'package.json'), 'utf8'));
  if (installed.version !== expected.devDependencies['@redocly/cli']) {
    throw new Error('Installed Redocly does not match the reviewed package.json pin; run pnpm install --frozen-lockfile.');
  }
  return resolve(dirname(packagePath), installed.bin.redocly);
}

export function toolEnvironment(environment = process.env) {
  const result = { ...environment };
  // Yargs accepts command options through this prefix. A developer's ambient
  // skip-rule/config/output settings must not change the repository gate.
  for (const key of Object.keys(result)) {
    if (key.toUpperCase().startsWith('REDOCLY_CLI_')) delete result[key];
  }
  result.REDOCLY_TELEMETRY = 'off';
  result.REDOCLY_SUPPRESS_UPDATE_NOTICE = 'true';
  return result;
}

export function runRedocly(args) {
  const result = spawnSync(process.execPath, [redoclyCli(), ...args], {
    cwd: repositoryRoot,
    stdio: 'inherit',
    env: toolEnvironment(),
  });
  assertSuccessfulRedocly(result);
}

export function assertSuccessfulRedocly(result) {
  if (result.error) throw new Error('Could not start the pinned Redocly CLI.', { cause: result.error });
  if (result.status !== 0) {
    throw new Error(`Redocly did not complete successfully (${result.signal ?? result.status ?? 'unknown exit'}).`);
  }
}

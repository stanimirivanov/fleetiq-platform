import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const require = createRequire(import.meta.url);
const packagePath = require.resolve('@redocly/cli/package.json');
const { bin } = JSON.parse(readFileSync(packagePath, 'utf8'));
const cli = resolve(dirname(packagePath), bin.redocly);
const source = join(root, 'contracts/http/openapi/openapi.yaml');
const config = join(root, 'contracts/http/redocly.yaml');
const committed = join(root, 'contracts/http/dist/openapi.json');
const temporaryDirectory = mkdtempSync(join(tmpdir(), 'fleetiq-openapi-'));
const generated = join(temporaryDirectory, 'openapi.json');

try {
  const result = spawnSync(
    process.execPath,
    [
      cli,
      'bundle',
      source,
      '--config',
      config,
      '--output',
      generated,
      '--component-renaming-conflicts-severity=error',
    ],
    {
      cwd: root,
      stdio: 'inherit',
      env: {
        ...process.env,
        REDOCLY_TELEMETRY: 'off',
        REDOCLY_SUPPRESS_UPDATE_NOTICE: 'true',
      },
    },
  );

  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);

  if (!readFileSync(generated).equals(readFileSync(committed))) {
    console.error('OpenAPI bundle is stale; run pnpm bundle:openapi and commit contracts/http/dist/openapi.json.');
    process.exitCode = 1;
  }
} finally {
  rmSync(temporaryDirectory, { recursive: true, force: true });
}

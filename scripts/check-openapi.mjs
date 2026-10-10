import { resolve } from 'node:path';
import { checkBundle } from './openapi/bundle.mjs';
import { assertRealSourcePath } from './openapi/layout.mjs';
import { repositoryRoot, runRedocly } from './openapi/tool.mjs';

if (process.argv.length !== 2) {
  console.error('Usage: node scripts/check-openapi.mjs');
  process.exitCode = 1;
} else {
  try {
    const source = resolve(repositoryRoot, 'contracts/http/openapi/openapi.yaml');
    assertRealSourcePath(source, repositoryRoot);
    checkBundle({
      source,
      config: resolve(repositoryRoot, 'contracts/http/redocly.yaml'),
      committed: resolve(repositoryRoot, 'contracts/http/dist/openapi.json'),
      run: runRedocly,
    });
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

import { resolve } from 'node:path';
import { assertRealSourcePath } from './openapi/layout.mjs';
import { runRedocly, repositoryRoot } from './openapi/tool.mjs';

if (process.argv.length !== 2) {
  console.error('Usage: node scripts/lint-openapi-layout.mjs');
  process.exitCode = 1;
} else {
  try {
    const source = resolve(repositoryRoot, 'contracts/http/openapi/openapi.yaml');
    assertRealSourcePath(source, repositoryRoot);
    runRedocly([
      'lint',
      source,
      '--config',
      resolve(repositoryRoot, 'contracts/http/redocly.yaml'),
      '--lint-config=error',
    ]);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

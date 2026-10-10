import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { checkBundle } from '../openapi/bundle.mjs';
import { assertSuccessfulRedocly, toolEnvironment } from '../openapi/tool.mjs';

function fixture(context) {
  const root = mkdtempSync(join(tmpdir(), 'fleetiq-openapi-bundle-'));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const temporaryParent = join(root, 'temporary');
  mkdirSync(temporaryParent);
  const committed = join(root, 'committed.json');
  const bytes = Buffer.from('{"synthetic":"contract"}\n');
  writeFileSync(committed, bytes);
  return {
    root, temporaryParent, committed, bytes,
    check(run) {
      return checkBundle({ source: join(root, 'source.yaml'), config: join(root, 'config.yaml'), committed, temporaryParent, run });
    },
    assertClean() {
      assert.deepEqual(readdirSync(temporaryParent), []);
      assert.deepEqual(readFileSync(committed), bytes);
    },
  };
}

function outputPath(args) { return args[args.indexOf('--output') + 1]; }

test('accepts equal bytes without changing the committed bundle or leaking temporary output', (context) => {
  const sample = fixture(context);
  sample.check((args) => {
    assert.equal(args[0], 'bundle');
    assert.ok(args.includes('--component-renaming-conflicts-severity=error'));
    assert.notEqual(outputPath(args), sample.committed);
    writeFileSync(outputPath(args), sample.bytes);
  });
  sample.assertClean();
});

test('rejects stale bytes even when both JSON documents mean the same thing', (context) => {
  const sample = fixture(context);
  assert.throws(() => sample.check((args) => writeFileSync(outputPath(args), '{ "synthetic": "contract" }\n')), /bundle is stale/u);
  sample.assertClean();
});

for (const [name, result, diagnostic] of [
  ['launch error', { error: new Error('Synthetic launch failure') }, /Could not start/u],
  ['nonzero exit', { status: 2 }, /successfully \(2\)/u],
  ['signal cancellation', { status: null, signal: 'SIGTERM' }, /SIGTERM/u],
]) {
  test(`cleans up and preserves the committed bundle after a ${name}`, (context) => {
    const sample = fixture(context);
    assert.throws(() => sample.check((args) => {
      writeFileSync(outputPath(args), 'partial synthetic output');
      assertSuccessfulRedocly(result);
    }), diagnostic);
    sample.assertClean();
  });
}

test('missing generated output is a failure and still cleans up', (context) => {
  const sample = fixture(context);
  assert.throws(() => sample.check(() => {}), /ENOENT/u);
  sample.assertClean();
});

test('a missing committed artifact fails without creating or replacing it', (context) => {
  const sample = fixture(context);
  rmSync(sample.committed);
  assert.throws(() => sample.check((args) => writeFileSync(outputPath(args), sample.bytes)), /ENOENT/u);
  assert.equal(existsSync(sample.committed), false);
  assert.deepEqual(readdirSync(sample.temporaryParent), []);
});

test('each comparison uses a fresh output directory', (context) => {
  const sample = fixture(context);
  const outputs = [];
  for (let index = 0; index < 2; index += 1) {
    sample.check((args) => { outputs.push(outputPath(args)); writeFileSync(outputPath(args), sample.bytes); });
  }
  assert.notEqual(outputs[0], outputs[1]);
  sample.assertClean();
});

test('ambient Redocly CLI arguments cannot skip the repository rule or redirect output', () => {
  const environment = toolEnvironment({ PATH: 'synthetic-path', REDOCLY_CLI_LINT_SKIP_RULE: 'fleetiq/layout', redocly_cli_bundle_output: 'unexpected-output', REDOCLY_CLI_LINT_CONFIG: 'unexpected-config' });
  assert.deepEqual(environment, { PATH: 'synthetic-path', REDOCLY_TELEMETRY: 'off', REDOCLY_SUPPRESS_UPDATE_NOTICE: 'true' });
});

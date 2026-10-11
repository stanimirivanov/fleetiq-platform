import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { checkBundle } from '../openapi/bundle.mjs';
import { assertSuccessfulRedocly, repositoryRoot, toolEnvironment } from '../openapi/tool.mjs';

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

function checkoutFixture(context, autocrlf, policy = readFileSync(join(repositoryRoot, '.gitattributes'))) {
  const sample = fixture(context);
  const repository = join(sample.root, 'source-repository');
  const checkout = join(sample.root, `checkout-${autocrlf}`);
  const hooks = join(sample.root, 'empty-hooks');
  const template = join(sample.root, 'empty-template');
  const config = join(sample.root, 'empty-config');
  const attributes = join(sample.root, 'empty-global-attributes');
  mkdirSync(hooks);
  mkdirSync(template);
  writeFileSync(config, '');
  writeFileSync(attributes, '');
  const environment = { ...process.env };
  // A caller's worktree, index, configuration injection, and system/global
  // attribute rules must not redirect or change this disposable Git fixture.
  for (const key of Object.keys(environment)) if (key.toUpperCase().startsWith('GIT_')) delete environment[key];
  environment.GIT_CONFIG_NOSYSTEM = '1';
  environment.GIT_CONFIG_GLOBAL = config;
  environment.GIT_ATTR_NOSYSTEM = '1';
  const settings = [
    '-c', `core.hooksPath=${hooks}`,
    '-c', `core.attributesFile=${attributes}`,
    '-c', `init.templateDir=${template}`,
    '-c', 'commit.gpgSign=false',
    '-c', 'user.name=FleetIQ harness fixture',
    '-c', 'user.email=harness-fixture@fleetiq.invalid',
  ];
  const git = (args) => {
    const result = spawnSync('git', [...settings, ...args], { cwd: sample.root, env: environment, encoding: 'utf8', timeout: 20_000 });
    assert.equal(result.error, undefined, result.error?.message);
    assert.equal(result.status, 0, `Disposable Git fixture setup failed: ${result.stdout}${result.stderr}`);
    return result.stdout;
  };
  git(['-c', 'core.autocrlf=input', 'init', '--initial-branch=main', repository]);
  const contractDirectory = join(repository, 'contracts/http/dist');
  mkdirSync(contractDirectory, { recursive: true });
  writeFileSync(join(repository, '.gitattributes'), policy);
  writeFileSync(join(contractDirectory, 'openapi.json'), sample.bytes);
  const control = Buffer.from('Synthetic checkout control\nSecond control line\n');
  writeFileSync(join(repository, 'control.txt'), control);
  git(['-c', 'core.autocrlf=input', '-C', repository, 'add', '--', '.gitattributes', 'contracts/http/dist/openapi.json', 'control.txt']);
  git(['-c', 'core.autocrlf=input', '-C', repository, 'commit', '-m', 'Create synthetic checkout fixture']);
  // --no-checkout ensures the first materialization uses the policy under test;
  // --no-hardlinks keeps every object file within this fixture independently owned.
  git(['clone', '--no-checkout', '--no-hardlinks', '--', repository, checkout]);
  // Reset only this disposable control's eol attribute. A repository-wide LF
  // policy must still protect the artifact while the control exercises autocrlf.
  mkdirSync(join(checkout, '.git/info'), { recursive: true });
  writeFileSync(join(checkout, '.git/info/attributes'), '/control.txt text !eol\n');
  git(['-c', `core.autocrlf=${autocrlf}`, '-C', checkout, 'checkout', '--force', 'HEAD']);
  const effective = git(['-C', checkout, 'check-attr', '-z', 'text', 'eol', '--', 'control.txt', 'contracts/http/dist/openapi.json']).split('\0');
  assert.deepEqual(effective.slice(0, 6), ['control.txt', 'text', 'set', 'control.txt', 'eol', 'unspecified']);
  const expectedControl = autocrlf === 'true' ? Buffer.from(control.toString('utf8').replaceAll('\n', '\r\n')) : control;
  // An ineffective checkout or a lingering LF override must fail setup rather
  // than let the artifact assertion pass without exercising conversion.
  assert.deepEqual(readFileSync(join(checkout, 'control.txt')), expectedControl);
  const committed = join(checkout, 'contracts/http/dist/openapi.json');
  return {
    ...sample,
    checkout,
    artifactEol: effective[11],
    checkCheckout() {
      return checkBundle({
        source: join(checkout, 'synthetic-source.yaml'),
        config: join(checkout, 'synthetic-config.yaml'),
        committed,
        temporaryParent: sample.temporaryParent,
        run: (args) => writeFileSync(outputPath(args), sample.bytes),
      });
    },
  };
}

for (const [name, policy] of [
  ['repository policy', undefined],
  ['narrow artifact LF policy', '* text=auto\n/contracts/http/dist/openapi.json text eol=lf\n'],
]) {
  for (const autocrlf of ['true', 'input']) {
    test(`fresh Git checkout keeps canonical JSON under core.autocrlf=${autocrlf} with ${name}`, (context) => {
      const sample = checkoutFixture(context, autocrlf, policy);
      assert.equal(sample.artifactEol, 'lf');
      const committed = join(sample.checkout, 'contracts/http/dist/openapi.json');
      assert.deepEqual(readFileSync(committed), sample.bytes, 'The generated JSON artifact must remain LF even when Git converts the isolated control to CRLF.');
      sample.checkCheckout();
      assert.deepEqual(readFileSync(committed), sample.bytes);
      sample.assertClean();
    });
  }
}

test('an unprotected Git checkout fails strict freshness even with the isolated control', (context) => {
  const sample = checkoutFixture(context, 'true', '* text=auto\n');
  assert.equal(sample.artifactEol, 'unspecified');
  const committed = join(sample.checkout, 'contracts/http/dist/openapi.json');
  const crlf = Buffer.from(sample.bytes.toString('utf8').replaceAll('\n', '\r\n'));
  assert.deepEqual(readFileSync(committed), crlf);
  assert.throws(() => sample.checkCheckout(), /bundle is stale/u);
  assert.deepEqual(readFileSync(committed), crlf);
  sample.assertClean();
});

test('a CRLF committed artifact remains stale under the strict byte comparator', (context) => {
  const sample = fixture(context);
  const crlf = Buffer.from(sample.bytes.toString('utf8').replaceAll('\n', '\r\n'));
  writeFileSync(sample.committed, crlf);
  assert.throws(() => sample.check((args) => writeFileSync(outputPath(args), sample.bytes)), /bundle is stale/u);
  assert.deepEqual(readFileSync(sample.committed), crlf);
  assert.deepEqual(readdirSync(sample.temporaryParent), []);
});

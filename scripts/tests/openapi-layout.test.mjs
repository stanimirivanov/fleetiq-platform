import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { test } from 'node:test';
import { assertRealSourcePath, expandedSchemaLines, inspectLayout } from '../openapi/layout.mjs';
import { redoclyCli, repositoryRoot, toolEnvironment } from '../openapi/tool.mjs';
import { assertReviewedIgnorePolicy } from '../openapi/ignore-policy.mjs';

function fixture(context) {
  const directory = mkdtempSync(join(tmpdir(), 'fleetiq-openapi-layout-'));
  context.after(() => rmSync(directory, { recursive: true, force: true }));
  const root = join(directory, 'openapi');
  const documents = new Map();
  function write(name, value) {
    const path = join(root, name);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, JSON.stringify(value, null, 2));
    documents.set(name, value);
  }
  write('openapi.yaml', {
    openapi: '3.1.1',
    info: { title: 'Synthetic harness fixture', version: '1' },
    paths: { '/assets': { $ref: './paths/assets/catalogue.yaml' } },
    components: {
      schemas: { Asset: { $ref: './components/schemas/Asset.yaml' } },
      parameters: { limit: { $ref: './components/parameters/limit.yaml' } },
      responses: { Ok: { $ref: './components/responses/Ok.yaml' } },
    },
  });
  write('paths/assets/catalogue.yaml', {
    get: {
      operationId: 'listAssets',
      parameters: [{ $ref: '../../components/parameters/limit.yaml' }],
      responses: { 200: { $ref: '../../components/responses/Ok.yaml' } },
    },
  });
  write('components/schemas/Asset.yaml', { type: 'object', properties: { id: { type: 'string' } } });
  write('components/parameters/limit.yaml', { name: 'limit', in: 'query', schema: { type: 'integer', minimum: 1 } });
  write('components/responses/Ok.yaml', {
    description: 'Synthetic success',
    content: { 'application/json': { schema: { $ref: '../schemas/Asset.yaml' } } },
  });
  return {
    directory, root, documents, write,
    document: documents.get('openapi.yaml'),
    path: documents.get('paths/assets/catalogue.yaml'),
    inspect() {
      return inspectLayout({
        rootDirectory: root,
        document: documents.get('openapi.yaml'),
        resolveDocument: ({ $ref }) => documents.get($ref.slice(2).split('#', 1)[0]),
      });
    },
    save() { for (const [name, document] of documents) write(name, document); },
    lint(config = resolve(repositoryRoot, 'contracts/http/redocly.yaml')) {
      return spawnSync(process.execPath, [redoclyCli(), 'lint', join(root, 'openapi.yaml'), '--config', config, '--format=json', '--lint-config=error'], {
        cwd: directory, encoding: 'utf8', env: toolEnvironment(), timeout: 30_000,
      });
    },
  };
}

function messages(result) { return result.issues.map((issue) => issue.message).join('\n'); }
function smallSchema() {
  return { type: 'object', description: 'Synthetic schema', properties: Object.fromEntries(Array.from({ length: 6 }, (_, index) => [`field${index}`, { type: 'string' }])) };
}
function setPathSchema(fixture, schema) {
  fixture.path.get.responses[200] = { description: 'Synthetic success', content: { 'application/json': { schema } } };
}

function assertLiveRuleFailure(result, message) {
  assert.equal(result.error, undefined, result.error?.message);
  assert.notEqual(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /fleetiq\/layout/u, result.stdout + result.stderr);
  assert.match(result.stdout, message);
}

test('accepts a fully registered modular tree and local schema fragment', (context) => {
  const sample = fixture(context);
  sample.documents.get('components/responses/Ok.yaml').content['application/json'].schema.$ref = '../schemas/Asset.yaml#/properties/id';
  assert.equal(sample.inspect().files, 5);
  assert.deepEqual(sample.inspect().issues, []);
});

test('a literal $ref in documentation is data, not a structural reference', (context) => {
  const sample = fixture(context);
  sample.path.get.description = '$ref: "https://not-a-real-reference.invalid/schema.yaml"';
  assert.deepEqual(sample.inspect().issues, []);
});

test('extension references cannot pretend to register an omitted root path', (context) => {
  const sample = fixture(context);
  sample.document['x-fake-index'] = sample.document.paths['/assets'];
  sample.document.paths = {};
  assert.match(messages(sample.inspect()), /exactly one root path\/component registration, found 0/u);
});

test('rejects duplicate root registrations', (context) => {
  const sample = fixture(context);
  sample.document.paths['/assets-again'] = sample.document.paths['/assets'];
  assert.match(messages(sample.inspect()), /registration, found 2/u);
});

test('rejects inline root path items and component entries', (context) => {
  const sample = fixture(context);
  sample.document.paths['/assets'] = sample.path;
  sample.document.components.schemas.Asset = { type: 'string' };
  assert.match(messages(sample.inspect()), /root path\/component entries must contain exactly one whole-file \$ref/u);
});

test('rejects references in the wrong component category', (context) => {
  const sample = fixture(context);
  delete sample.document.components.schemas.Asset;
  sample.document.components.responses.Asset = { $ref: './components/schemas/Asset.yaml' };
  assert.match(messages(sample.inspect()), /register this resource under components\/responses/u);
});

test('rejects a component key that disagrees with its filename', (context) => {
  const sample = fixture(context);
  sample.document.components.schemas.Other = sample.document.components.schemas.Asset;
  delete sample.document.components.schemas.Asset;
  assert.match(messages(sample.inspect()), /index key must match its category and filename/u);
});

test('root registration must name the whole file', (context) => {
  const sample = fixture(context);
  sample.document.components.schemas.Asset.$ref += '#/properties/id';
  assert.match(messages(sample.inspect()), /whole file without a fragment/u);
});

for (const [name, reference, diagnostic] of [
  ['remote URL', 'https://example.invalid/schema.yaml', /explicit relative/u],
  ['absolute path', '/tmp/schema.yaml', /explicit relative/u],
  ['Windows drive path', 'C:/schema.yaml', /explicit relative/u],
  ['backslash', '..\\schemas\\Asset.yaml', /forward-slash/u],
  ['query options', '../schemas/Asset.yaml?bypass=1', /query options/u],
  ['URL-encoded traversal', '../schemas/%2e%2e/Asset.yaml', /URL encoding/u],
  ['directory escape', '../../../../outside.yaml', /below openapi/u],
  ['redundant traversal', '../schemas/../schemas/Asset.yaml', /directly target/u],
  ['missing target', '../schemas/Absent.yaml', /absent/u],
  ['filename casing drift', '../schemas/asset.yaml', /filename casing/u],
  ['non-string value', { file: 'Asset.yaml' }, /explicit relative/u],
]) {
  test(`rejects ${name} in a parsed nested reference`, (context) => {
    const sample = fixture(context);
    sample.documents.get('components/responses/Ok.yaml').content['application/json'].schema.$ref = reference;
    assert.match(messages(sample.inspect()), diagnostic);
  });
}

test('rejects an unregistered YAML file and unsupported source extensions', (context) => {
  const sample = fixture(context);
  sample.write('components/schemas/Unregistered.yaml', { type: 'string' });
  sample.write('paths/assets/hidden.json', {});
  assert.match(messages(sample.inspect()), /registration, found 0/u);
  assert.match(messages(sample.inspect()), /regular \.yaml files/u);
});

test('requires a real root entrypoint and a readable source tree', (context) => {
  const sample = fixture(context);
  unlinkSync(join(sample.root, 'openapi.yaml'));
  assert.match(messages(sample.inspect()), /openapi.yaml must be a regular source file/u);
  const missing = inspectLayout({ rootDirectory: join(sample.directory, 'missing'), document: {}, resolveDocument() {} });
  assert.match(messages(missing), /could not be read/u);
});

test('rejects a registered document that cannot be parsed/resolved', (context) => {
  const sample = fixture(context);
  sample.documents.delete('components/schemas/Asset.yaml');
  assert.match(messages(sample.inspect()), /could not be parsed\/resolved/u);
});

test('path and operation parameters must reference registered parameter components', (context) => {
  const sample = fixture(context);
  sample.path.get.parameters = [{ name: 'limit', in: 'query', schema: { type: 'integer' } }];
  assert.match(messages(sample.inspect()), /parameters must reference registered parameter files/u);
});

test('the inline schema budget accepts exactly fifteen expanded lines', (context) => {
  const sample = fixture(context);
  assert.equal(expandedSchemaLines(smallSchema()), 15);
  setPathSchema(sample, smallSchema());
  assert.deepEqual(sample.inspect().issues, []);
});

test('nested schema structure, not physical line formatting, triggers extraction', (context) => {
  const sample = fixture(context);
  const schema = smallSchema();
  schema.properties.extra = { type: 'string' };
  setPathSchema(sample, schema);
  assert.match(messages(sample.inspect()), /exceeds 15 expanded YAML lines/u);
});

test('repeated aliases count at each use while recursive aliases fail without hanging', (context) => {
  const shared = { type: 'string' };
  assert.equal(expandedSchemaLines({ properties: { one: shared, two: shared } }), 5);
  const recursive = { type: 'object' };
  recursive.properties = recursive;
  assert.equal(expandedSchemaLines(recursive), Infinity);
  const sample = fixture(context);
  sample.path.get.callbacks = { recursive: { callback: sample.path } };
  assert.match(messages(sample.inspect()), /recursive YAML aliases/u);
});

test('rejects real directory symlinks/junctions on every supported host', (context) => {
  const sample = fixture(context);
  const outside = join(sample.directory, 'outside');
  mkdirSync(outside);
  const linked = join(sample.root, 'paths', 'linked');
  symlinkSync(outside, linked, process.platform === 'win32' ? 'junction' : 'dir');
  assert.equal(lstatSync(linked).isSymbolicLink(), true);
  assert.match(messages(sample.inspect()), /symlinks or junctions/u);
  assert.deepEqual(readdirSync(outside), []);
});

test('rejects the modular root itself when it is a directory symlink/junction', (context) => {
  const sample = fixture(context);
  const linked = join(sample.directory, 'linked-root');
  symlinkSync(sample.root, linked, process.platform === 'win32' ? 'junction' : 'dir');
  const result = inspectLayout({ rootDirectory: linked, document: sample.document, resolveDocument() {} });
  assert.match(messages(result), /real directories/u);
});

test('the pinned parser accepts quoted keys, flow references, comments, and block scalars', (context) => {
  const sample = fixture(context);
  writeFileSync(join(sample.root, 'paths/assets/catalogue.yaml'), `get:\n  operationId: listAssets\n  description: |\n    A literal example: $ref: "https://example.invalid/example.yaml"\n  parameters:\n    - { '$ref': '../../components/parameters/limit.yaml' } # valid YAML\n  responses:\n    '200': { "$ref": "../../components/responses/Ok.yaml" }\n`);
  const result = sample.lint();
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stdout + result.stderr);
});

test('live repository configuration detects an oversized flow schema', (context) => {
  const sample = fixture(context);
  const schema = smallSchema();
  schema.properties.extra = { type: 'string' };
  setPathSchema(sample, schema);
  sample.save();
  // The path document is JSON on one physical line, which is also valid YAML.
  writeFileSync(join(sample.root, 'paths/assets/catalogue.yaml'), JSON.stringify(sample.path));
  assertLiveRuleFailure(sample.lint(), /exceeds 15 expanded YAML lines/u);
});

test('live configuration rejects inline parameters hidden in flow syntax', (context) => {
  const sample = fixture(context);
  sample.path.get.parameters = [{ name: 'limit', in: 'query', schema: { type: 'integer' } }];
  sample.save();
  assertLiveRuleFailure(sample.lint(), /parameters must reference/u);
});

test('live configuration rejects a fake registration outside the root indexes', (context) => {
  const sample = fixture(context);
  sample.document['x-fake-index'] = sample.document.components.schemas.Asset;
  delete sample.document.components.schemas.Asset;
  sample.save();
  assertLiveRuleFailure(sample.lint(), /registration, found 0/u);
});

test('live parser preserves reference-chain edges instead of hiding traversal', (context) => {
  const sample = fixture(context);
  sample.write('components/schemas/Asset.yaml', { $ref: '../../../outside.yaml' });
  writeFileSync(join(sample.directory, 'outside.yaml'), '{"type":"string"}');
  assertLiveRuleFailure(sample.lint(), /whole-file aliases require explicit composition/u);
});

test('malformed YAML fails the ordinary parser gate', (context) => {
  const sample = fixture(context);
  writeFileSync(join(sample.root, 'paths/assets/catalogue.yaml'), 'get: [unterminated');
  const result = sample.lint();
  assert.equal(result.error, undefined);
  assert.notEqual(result.status, 0, result.stdout + result.stderr);
});

test('rejects a junction/symlink in a source parent before invoking the parser', (context) => {
  const sample = fixture(context);
  const redirected = join(sample.directory, 'redirected-http');
  symlinkSync(sample.directory, redirected, process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => assertRealSourcePath(join(redirected, 'openapi/openapi.yaml'), sample.directory), /never symlinks or junctions/u);
  assert.doesNotThrow(() => assertRealSourcePath(join(sample.root, 'openapi.yaml'), sample.directory));
});

test('rejects a source path that escapes the specified checkout boundary', (context) => {
  const sample = fixture(context);
  assert.throws(() => assertRealSourcePath(join(sample.directory, '..', 'outside.yaml'), sample.directory), /below the checkout/u);
});

test('a whole-file schema alias cannot hide the original owning source', (context) => {
  const sample = fixture(context);
  sample.write('components/schemas/Asset.yaml', { $ref: './Other.yaml' });
  sample.write('components/schemas/Other.yaml', { type: 'string' });
  sample.document.components.schemas.Other = { $ref: './components/schemas/Other.yaml' };
  sample.save();
  assertLiveRuleFailure(sample.lint(), /whole-file aliases require explicit composition/u);
});
test('fragment escaping is independent of canonical file path spelling', (context) => {
  const sample = fixture(context);
  sample.documents.get('components/responses/Ok.yaml').content['application/json'].schema.$ref = '../schemas/Asset.yaml#/properties/name%20with%20space';
  assert.deepEqual(sample.inspect().issues, []);
});

test('the real resolver accepts escaped JSON Pointer property names', (context) => {
  const sample = fixture(context);
  sample.write('components/schemas/Asset.yaml', { type: 'object', properties: { 'id/part~tail': { type: 'string' } } });
  sample.documents.get('components/responses/Ok.yaml').content['application/json'].schema.$ref = '../schemas/Asset.yaml#/properties/id~1part~0tail';
  sample.save();
  const result = sample.lint();
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stdout + result.stderr);
});
test('parsed ignore policy preserves only the exact reviewed redirect pointers', (context) => {
  const sample = fixture(context);
  const config = join(sample.directory, 'redocly.yaml');
  const login = join(sample.root, 'paths/identity/login.yaml');
  const callback = join(sample.root, 'paths/identity/callback.yaml');
  assert.doesNotThrow(() => assertReviewedIgnorePolicy({
    [login]: { 'operation-2xx-response': new Set(['#/get/responses']) },
    [callback]: { 'operation-2xx-response': new Set(['#/get/responses']) },
  }, config));
  for (const ignore of [
    { [login]: { 'fleetiq/layout': new Set(['#/']) } },
    { [login]: { 'operation-2xx-response': new Set(['#/']) } },
    { [login]: { 'operation-2xx-response': new Set(['#/get/responses', '#/post/responses']) } },
    { [join(sample.root, 'paths/assets/catalogue.yaml')]: { 'operation-2xx-response': new Set(['#/get/responses']) } },
  ]) {
    assert.throws(() => assertReviewedIgnorePolicy(ignore, config), /structural findings cannot be suppressed/u);
  }
});

test('a real ignore file cannot suppress the mandatory layout rule', (context) => {
  const sample = fixture(context);
  sample.document['x-fake-index'] = sample.document.components.schemas.Asset;
  delete sample.document.components.schemas.Asset;
  sample.save();
  const plugin = resolve(repositoryRoot, 'scripts/openapi/redocly-plugin.mjs').replaceAll('\\', '/');
  const config = join(sample.directory, 'redocly.yaml');
  const sourceConfig = readFileSync(resolve(repositoryRoot, 'contracts/http/redocly.yaml'), 'utf8');
  writeFileSync(config, sourceConfig.replace('../../scripts/openapi/redocly-plugin.mjs', plugin));
  writeFileSync(join(sample.directory, '.redocly.lint-ignore.yaml'), "openapi/openapi.yaml:\n  fleetiq/layout:\n    - '#/'\n");
  const result = sample.lint(config);
  assert.equal(result.error, undefined);
  assert.notEqual(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout + result.stderr, /structural findings cannot be suppressed/u);
});
test('live parser leaves media examples and extension payloads opaque', (context) => {
  const sample = fixture(context);
  const payload = { $ref: 'business-record-123', parameters: ['business-data'], schema: { arbitrary: 'payload' } };
  sample.path.get['x-display-state'] = payload;
  sample.documents.get('components/responses/Ok.yaml').content['application/json'].example = payload;
  sample.save();
  assert.deepEqual(sample.inspect().issues, []);
  const result = sample.lint();
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stdout + result.stderr);
});

test('live parser rejects a parameter-shaped schema file used as an operation parameter', (context) => {
  const sample = fixture(context);
  sample.write('components/schemas/Asset.yaml', { type: 'object', example: { name: 'limit', in: 'query', schema: { type: 'integer' } } });
  sample.path.get.parameters = [{ $ref: '../../components/schemas/Asset.yaml#/example' }];
  sample.save();
  assertLiveRuleFailure(sample.lint(), /parameters must reference registered parameter files/u);
});
test('live schemas preserve reserved property names and opaque annotation values', (context) => {
  const sample = fixture(context);
  const payload = { $ref: 'business-record-123', parameters: ['business-data'], schema: { arbitrary: 'payload' } };
  sample.write('components/schemas/Asset.yaml', {
    type: 'object',
    properties: {
      $ref: { type: 'string' },
      schema: { type: 'object' },
      parameters: { type: 'array', items: { type: 'string' } },
    },
    const: payload, default: payload, enum: [payload], examples: [payload], 'x-fixture': payload,
  });
  sample.save();
  assert.deepEqual(sample.inspect().issues, []);
  const result = sample.lint();
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stdout + result.stderr);
});

test('named Example Object references resolve while Example.value stays opaque', (context) => {
  const sample = fixture(context);
  const media = sample.documents.get('components/responses/Ok.yaml').content['application/json'];
  media.examples = {
    baseline: { summary: 'Synthetic sample', value: { $ref: 'business-record-123', parameters: ['business-data'] } },
    reused: { $ref: './Ok.yaml#/content/application~1json/examples/baseline' },
  };
  sample.save();
  assert.deepEqual(sample.inspect().issues, []);
  const result = sample.lint();
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stdout + result.stderr);
});

for (const kind of ['named Example', 'nested Schema', 'operation Parameter']) {
  test(`missing real ${kind} references still fail the original resolver and structural rule`, (context) => {
    const sample = fixture(context);
    if (kind === 'named Example') {
      sample.documents.get('components/responses/Ok.yaml').content['application/json'].examples = { missing: { $ref: './Missing.yaml' } };
    } else if (kind === 'nested Schema') {
      sample.documents.get('components/schemas/Asset.yaml').properties.missing = { $ref: './Missing.yaml' };
    } else {
      sample.path.get.parameters = [{ $ref: '../../components/parameters/missing.yaml' }];
    }
    sample.save();
    const result = sample.lint();
    assertLiveRuleFailure(result, /absent/u);
    assert.match(result.stdout, /no-unresolved-refs/u);
  });
}

test('opaque payload metadata keeps normative field validators enabled', (context) => {
  const sample = fixture(context);
  sample.path.get.operationId = 123;
  sample.path.get.unexpectedField = 'must fail';
  sample.save();
  const result = sample.lint();
  assert.equal(result.error, undefined);
  assert.notEqual(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /"ruleId": "struct"/u);
  assert.match(result.stdout, /unexpectedField/u);
});
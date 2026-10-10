import { lstatSync, readdirSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { resourceKind, walkResource } from './traversal.mjs';

const componentDirectories = new Map([
  ['schemas', 'schemas'],
  ['parameters', 'parameters'],
  ['responses', 'responses'],
  ['headers', 'headers'],
  ['securitySchemes', 'security-schemes'],
]);
const kebabCase = /^[a-z0-9]+(?:-[a-z0-9]+)*$/u;
const isObject = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const pointer = (parts) => `#/${parts.map((part) => String(part).replaceAll('~', '~0').replaceAll('/', '~1')).join('/')}`;

// The budget counts expanded mapping/sequence entries, not physical source lines.
// A flow mapping, alias, comment, or long description cannot disguise structure.
export function expandedSchemaLines(value, ancestors = new Set()) {
  if (value === null || typeof value !== 'object') return 0;
  if (ancestors.has(value)) return Infinity;
  const nextAncestors = new Set(ancestors).add(value);
  return Object.values(value).reduce((count, child) => count + 1 + expandedSchemaLines(child, nextAncestors), 0);
}

/** Inspect parsed source documents; Redocly remains the YAML/OpenAPI authority. */
export function inspectLayout({ rootDirectory, document, resolveDocument }) {
  const root = resolve(rootDirectory);
  const entrypoint = join(root, 'openapi.yaml');
  const files = new Set();
  const registered = new Map();
  const documents = new Map([['openapi.yaml', document]]);
  const issues = [];
  const display = (file) => relative(root, file).split(sep).join('/');
  const report = (file, parts, message) => issues.push({ file: display(file), parts, message });

  function collect(directory) {
    // lstat intentionally rejects both Unix symlinks and Windows junctions.
    const metadata = lstatSync(directory);
    if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
      report(directory, [], 'source directories must be real directories, never symlinks or junctions');
      return;
    }
    for (const entry of readdirSync(directory, { withFileTypes: true }).sort((left, right) => left.name.localeCompare(right.name, 'en'))) {
      const file = join(directory, entry.name);
      const metadata = lstatSync(file);
      if (metadata.isSymbolicLink()) {
        report(file, [], 'source entries must not be symlinks or junctions');
      } else if (metadata.isDirectory()) {
        collect(file);
      } else if (!metadata.isFile() || !entry.name.endsWith('.yaml')) {
        report(file, [], 'the modular source tree contains only regular .yaml files');
      } else {
        files.add(file);
      }
    }
  }

  try {
    collect(root);
  } catch {
    report(entrypoint, [], 'the modular source tree could not be read');
  }
  if (!files.has(entrypoint)) report(entrypoint, [], 'openapi.yaml must be a regular source file');

  function referenceTarget(file, parts, reference) {
    const targetPath = typeof reference === 'string' ? reference.split('#', 1)[0] : '';
    if (typeof reference !== 'string' || !/^\.{1,2}\//u.test(targetPath) || /[\\?%]/u.test(targetPath)) {
      report(file, parts, '$ref must use an explicit relative forward-slash file path without URL encoding or query options');
      return undefined;
    }
    const segments = targetPath.split('/');
    let traversedDirectory = false;
    const redundant = segments.some((segment, index) => {
      if (segment === '' || (segment === '.' && index !== 0)) return true;
      if (segment === '..') return traversedDirectory;
      if (segment !== '.') traversedDirectory = true;
      return false;
    });
    const target = resolve(dirname(file), targetPath);
    const name = relative(root, target);
    if (redundant || name === '..' || name.startsWith(`..${sep}`) || !targetPath.endsWith('.yaml')) {
      report(file, parts, '$ref must directly target a YAML file below openapi/');
      return undefined;
    }
    // Match the collected spelling exactly even on case-insensitive Windows.
    if (!files.has(target)) {
      report(file, parts, `$ref target ${reference} is absent, nonregular, or has incorrect filename casing`);
      return undefined;
    }
    return target;
  }

  function indexReference(value, parts, expectedDirectory, expectedName) {
    if (!isObject(value) || typeof value.$ref !== 'string' || Object.keys(value).length !== 1) {
      report(entrypoint, parts, 'root path/component entries must contain exactly one whole-file $ref');
      return;
    }
    const target = referenceTarget(entrypoint, [...parts, '$ref'], value.$ref);
    if (!target) return;
    const name = display(target);
    if (value.$ref.includes('#')) report(entrypoint, parts, 'root registration must reference a whole file without a fragment');
    if (!name.startsWith(`${expectedDirectory}/`)) {
      report(entrypoint, parts, `register this resource under ${expectedDirectory}/`);
    }
    if (expectedName !== undefined && name !== `${expectedDirectory}/${expectedName}.yaml`) {
      report(entrypoint, parts, 'component index key must match its category and filename');
    }
    if (expectedDirectory === 'paths') {
      const segments = name.split('/');
      if (segments.length !== 3 || !kebabCase.test(segments[1]) || !kebabCase.test(segments[2].slice(0, -5))) {
        report(entrypoint, parts, 'path files use paths/<lowercase-kebab-domain>/<lowercase-kebab-resource>.yaml');
      }
    } else if (['components/parameters', 'components/headers'].includes(expectedDirectory) && !kebabCase.test(expectedName)) {
      report(entrypoint, parts, 'parameter/header filenames and index keys use lowercase kebab-case');
    }
    registered.set(target, (registered.get(target) ?? 0) + 1);
    if (!documents.has(name)) {
      try {
        const parsed = resolveDocument(value, target);
        if (parsed === undefined || (isObject(parsed) && typeof parsed.$ref === 'string' && Object.keys(parsed).length === 1)) throw new Error('unresolved or aliased document');
        documents.set(name, parsed);
      } catch {
        report(entrypoint, parts, `registered source ${name} could not be parsed/resolved as its own document; whole-file aliases require explicit composition`);
      }
    }
  }

  if (!isObject(document?.paths)) report(entrypoint, ['paths'], 'root paths must be an index of file references');
  for (const [name, value] of Object.entries(document?.paths ?? {})) {
    indexReference(value, ['paths', name], 'paths');
  }
  for (const [category, values] of Object.entries(document?.components ?? {})) {
    const directory = componentDirectories.get(category);
    if (directory === undefined || !isObject(values)) {
      report(entrypoint, ['components', category], 'component category must have an explicitly supported file index');
      continue;
    }
    for (const [name, value] of Object.entries(values)) {
      indexReference(value, ['components', category, name], `components/${directory}`, name);
    }
  }

  for (const file of files) {
    if (file === entrypoint) continue;
    const count = registered.get(file) ?? 0;
    if (count !== 1) report(file, [], `expected exactly one root path/component registration, found ${count}`);
  }

  for (const [name, value] of documents) {
    const kind = resourceKind(name);
    if (kind === undefined) continue;
    const file = join(root, name);
    walkResource(value, kind, {
      reference: (parts, reference) => referenceTarget(file, parts, reference),
      cycle: (parts) => report(file, parts, 'recursive YAML aliases are not supported; use explicit component references'),
      schema: (parts, schema) => {
        if (kind === 'PathItem' && expandedSchemaLines(schema) > 15) report(file, parts, 'inline schema exceeds 15 expanded YAML lines; extract and register a schema component');
      },
      parameter: (parts, parameter) => {
        const target = isObject(parameter) && typeof parameter.$ref === 'string' ? referenceTarget(file, [...parts, '$ref'], parameter.$ref) : undefined;
        if (!target || !display(target).startsWith('components/parameters/') || registered.get(target) !== 1) {
          report(file, parts, 'path/operation parameters must reference registered parameter files');
        }
      },
    });
  }
  const unique = [...new Map(issues.map((issue) => [formatIssue(issue), issue])).values()];
  return { files: files.size, issues: unique };
}

export function formatIssue(issue) {
  return `${issue.file}${pointer(issue.parts)}: ${issue.message}`;
}

/** Reject redirected source ancestors inside the checkout before parsing. */
export function assertRealSourcePath(source, checkout) {
  const boundary = resolve(checkout);
  const name = relative(boundary, resolve(source));
  if (name === '..' || name.startsWith(`..${sep}`) || name === '') throw new Error('OpenAPI source must be below the checkout.');
  let current = boundary;
  const segments = name.split(sep);
  for (const [index, segment] of segments.entries()) {
    current = join(current, segment);
    const metadata = lstatSync(current);
    const expected = index === segments.length - 1 ? metadata.isFile() : metadata.isDirectory();
    if (metadata.isSymbolicLink() || !expected) throw new Error('OpenAPI source ancestors inside the checkout must be regular directories/files, never symlinks or junctions.');
  }
}
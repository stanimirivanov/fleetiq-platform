import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('../contracts/http/openapi/', import.meta.url)));
const entrypoint = join(root, 'openapi.yaml');
const files = collectYamlFiles(root);
const registered = new Map();
const errors = [];

function collectYamlFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return collectYamlFiles(path);
    return entry.name.endsWith('.yaml') ? [path] : [];
  });
}

function display(path) {
  return relative(root, path).replaceAll(sep, '/');
}

for (const file of files) {
  const lines = readFileSync(file, 'utf8').split(/\r?\n/u);
  for (const [index, line] of lines.entries()) {
    const match = /^\s*\$ref:\s*"([^"]+)"\s*$/u.exec(line);
    if (!match) {
      if (/^\s*\$ref:/u.test(line)) {
        errors.push(`${display(file)}:${index + 1}: quote relative $ref paths with double quotes`);
      }
      continue;
    }

    const reference = match[1];
    const targetPath = reference.split('#', 1)[0];
    if (!/^\.{1,2}\//u.test(targetPath) || targetPath.includes('\\')) {
      errors.push(`${display(file)}:${index + 1}: $ref must use a relative forward-slash file path`);
      continue;
    }

    const target = resolve(dirname(file), targetPath);
    if (!target.startsWith(`${root}${sep}`) || !target.endsWith('.yaml')) {
      errors.push(`${display(file)}:${index + 1}: $ref must target a YAML file below openapi/`);
      continue;
    }
    try {
      if (!statSync(target).isFile()) throw new Error('not a file');
    } catch {
      errors.push(`${display(file)}:${index + 1}: $ref target ${reference} does not exist`);
    }

    if (file === entrypoint) {
      registered.set(target, (registered.get(target) ?? 0) + 1);
    }
  }

  if (display(file).startsWith('paths/')) {
    for (const [index, line] of lines.entries()) {
      const schema = /^(\s*)schema:\s*$/u.exec(line);
      if (!schema) continue;
      const indentation = schema[1].length;
      let bodyLines = 0;
      for (const body of lines.slice(index + 1)) {
        if (!body.trim() || body.trimStart().startsWith('#')) continue;
        if (body.match(/^\s*/u)[0].length <= indentation) break;
        bodyLines += 1;
      }
      if (bodyLines > 15) {
        errors.push(`${display(file)}:${index + 1}: inline schema exceeds 15 lines; extract a component`);
      }
    }
  }
}

for (const file of files) {
  if (file === entrypoint) continue;
  const name = display(file);
  if (!name.startsWith('paths/') && !name.startsWith('components/')) continue;
  const count = registered.get(file) ?? 0;
  if (count !== 1) {
    errors.push(`${name}: expected exactly one $ref registration in openapi.yaml, found ${count}`);
  }
}

if (errors.length > 0) {
  for (const error of errors) console.error(error);
  process.exitCode = 1;
} else {
  console.log(`OpenAPI layout valid: ${files.length - 1} registered files`);
}

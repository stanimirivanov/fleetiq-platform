import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { chmod, mkdir, mkdtemp, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Release archive digests are reviewed pins, not fetched trust-on-first-use values.
// Source: https://github.com/gitleaks/gitleaks/releases/tag/v8.30.1
const version = '8.30.1';
const releases = {
  'linux-x64': ['linux_x64.tar.gz', '551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb'],
  'linux-arm64': ['linux_arm64.tar.gz', 'e4a487ee7ccd7d3a7f7ec08657610aa3606637dab924210b3aee62570fb4b080'],
  'win32-x64': ['windows_x64.zip', 'd29144deff3a68aa93ced33dddf84b7fdc26070add4aa0f4513094c8332afc4e'],
  'win32-arm64': ['windows_arm64.zip', 'b95f5e4f5c425cedca7ee203d9afd29597e692c4924a12ed42f970537c72cc0f'],
};
const release = releases[`${process.platform}-${process.arch}`];
if (!release) throw new Error('Pinned Gitleaks installation supports Linux and Windows x64/arm64.');
const [suffix, digest] = release;
const archiveName = `gitleaks_${version}_${suffix}`;
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const destination = join(root, 'target', 'tools', 'gitleaks');
const temporary = await mkdtemp(join(tmpdir(), 'fleetiq-gitleaks-install-'));
try {
  const response = await fetch(`https://github.com/gitleaks/gitleaks/releases/download/v${version}/${archiveName}`, {
    signal: AbortSignal.timeout(120_000),
  });
  if (!response.ok) throw new Error(`Official Gitleaks archive download failed (${response.status}).`);
  const bytes = Buffer.from(await response.arrayBuffer());
  if (createHash('sha256').update(bytes).digest('hex') !== digest) {
    throw new Error('Gitleaks archive checksum mismatch; nothing was installed.');
  }
  const archive = join(temporary, archiveName);
  await writeFile(archive, bytes);
  const extracted = join(temporary, 'extracted');
  await mkdir(extracted);
  const result = spawnSync('tar', ['-xf', archive, '-C', extracted], { encoding: 'utf8' });
  if (result.error || result.status !== 0) throw new Error('Gitleaks archive extraction failed; install a tar executable.');
  const binaryName = process.platform === 'win32' ? 'gitleaks.exe' : 'gitleaks';
  const binary = join(extracted, binaryName);
  await chmod(binary, 0o755);
  const probe = spawnSync(binary, ['version'], { encoding: 'utf8' });
  if (probe.error || probe.status !== 0 || probe.stdout.trim() !== version) {
    throw new Error('Extracted Gitleaks version does not match the reviewed pin.');
  }
  // Copy only the executable. The archive and extraction never enter the repository.
  await mkdir(destination, { recursive: true });
  const pending = join(destination, `${binaryName}.pending`);
  await writeFile(pending, await readFile(binary), { mode: 0o755 });
  await rename(pending, join(destination, binaryName));
  console.log(`Installed checksum-verified Gitleaks ${version} in target/tools/gitleaks.`);
} finally {
  await rm(temporary, { recursive: true, force: true });
}

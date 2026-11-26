import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { linuxPayloadComplete } from '../lib/linux.js';

assert.equal(process.platform, 'linux', 'Stage Linux payloads on Linux.');
assert.equal(process.arch, 'x64', 'The certified Linux build target is x64.');

const repository = fileURLToPath(new URL('../../../', import.meta.url));
const pkg = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
const targetSubdir = process.env.TARGET || 'x86_64-unknown-linux-gnu';
const debDir = resolve(repository, 'target', targetSubdir, 'release', 'bundle', 'deb');
const debFile =
  process.argv[2] ??
  (() => {
    const found = readdirSync(debDir).find((name) => name.endsWith('.deb'));
    assert.ok(found, `No .deb package found in ${debDir}`);
    return join(debDir, found);
  })();

const output = resolve(
  repository,
  process.env.TERMEXO_FLEET_LINUX_ARTIFACT_DIR ?? `artifacts/${targetSubdir}`,
);
const field = (name) =>
  execFileSync('dpkg-deb', ['--field', debFile, name], { encoding: 'utf8' }).trim();
assert.equal(field('Version'), pkg.version, 'Debian package version mismatch');
assert.equal(field('Architecture'), 'amd64', 'Debian package architecture mismatch');

const temporary = mkdtempSync(join(tmpdir(), 'termexo-fleet-linux-stage-'));
try {
  execFileSync('dpkg-deb', ['-x', debFile, temporary]);
  assert(
    linuxPayloadComplete(temporary),
    'The Debian package is missing the complete executable/runtime payload (usr/bin/termexo-fleet)',
  );
  const header = readFileSync(join(temporary, 'usr/bin/termexo-fleet')).subarray(0, 20);
  assert.equal(header.subarray(0, 4).toString(), '\x7fELF', 'usr/bin/termexo-fleet: invalid ELF');
  assert.equal(header[4], 2, 'usr/bin/termexo-fleet: expected ELF64');
  assert.equal(header[5], 1, 'usr/bin/termexo-fleet: expected little endian');
  assert.equal(header.readUInt16LE(18), 62, 'usr/bin/termexo-fleet: expected x86_64');

  mkdirSync(output, { recursive: true });
  const archiveName = `Termexo-Fleet_${pkg.version}_linux_x64.tar.gz`;
  execFileSync('tar', ['-czf', join(output, archiveName), '-C', temporary, 'usr']);

  const debName = `Termexo-Fleet_${pkg.version}_amd64.deb`;
  copyFileSync(debFile, join(output, debName));

  const sha256 = (file) => createHash('sha256').update(readFileSync(file)).digest('hex');
  const files = [debName, archiveName].map((name) => ({
    name,
    sha256: sha256(join(output, name)),
  }));
  writeFileSync(
    join(output, 'SHA256SUMS.txt'),
    files.map(({ name, sha256: hash }) => `${hash}  ${name}\n`).join(''),
  );
  const commit =
    process.env.GITHUB_SHA ??
    execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repository, encoding: 'utf8' }).trim();
  const dirty =
    !process.env.GITHUB_SHA &&
    Boolean(
      execFileSync('git', ['status', '--porcelain'], { cwd: repository, encoding: 'utf8' }).trim(),
    );
  writeFileSync(
    join(output, 'build-info.json'),
    `${JSON.stringify({ version: pkg.version, commit, dirty, target: targetSubdir, files }, null, 2)}\n`,
  );

  const vendor = fileURLToPath(
    new URL('../vendor/linux-x64/Termexo-Fleet.tar.gz', import.meta.url),
  );
  mkdirSync(dirname(vendor), { recursive: true });
  copyFileSync(join(output, archiveName), vendor);
  console.log(`Staged Linux x64 application and resources: ${output}`);
} finally {
  rmSync(temporary, { recursive: true, force: true });
}

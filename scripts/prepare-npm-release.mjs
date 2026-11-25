#!/usr/bin/env node
/**
 * Computes SHA-256 checksums for all release assets, writes SHA256SUMS.txt,
 * updates packages/npm/release-manifest.json, and copies root README.md into
 * packages/npm/README.md before `npm publish`.
 *
 * Usage:
 *   node scripts/prepare-npm-release.mjs --version 0.2.0 --tag v0.2.0 --assets-dir release-assets
 */
import { createHash } from 'node:crypto';
import { copyFile, readdir, readFile, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));

function getArg(flag, fallback = '') {
  const idx = process.argv.indexOf(flag);
  return idx >= 0 && process.argv[idx + 1] ? process.argv[idx + 1] : fallback;
}

const version = getArg('--version').replace(/^v/, '').trim();
const tag = getArg('--tag', version ? `v${version}` : '').trim();
const repo = getArg('--repo', 'gemron/Termexo-Fleet').trim();
const assetsDir = path.resolve(root, getArg('--assets-dir', 'release-assets'));

if (!version || !tag) {
  console.error('Usage: node scripts/prepare-npm-release.mjs --version <ver> --tag <tag> --assets-dir <dir>');
  process.exit(1);
}

const PORTABLE_TARGETS = {
  'windows-x64': {
    filename: 'termexo-fleet-windows-x64.zip',
    binaryRelativePath: 'termexo-fleet.exe',
  },
  'macos-arm64': {
    filename: 'termexo-fleet-macos-arm64.tar.gz',
    binaryRelativePath: 'Termexo Fleet.app/Contents/MacOS/termexo-fleet',
  },
  'macos-x64': {
    filename: 'termexo-fleet-macos-x64.tar.gz',
    binaryRelativePath: 'Termexo Fleet.app/Contents/MacOS/termexo-fleet',
  },
  'linux-x64': {
    filename: 'termexo-fleet-linux-x64.tar.gz',
    binaryRelativePath: 'termexo-fleet',
  },
};

async function sha256File(filePath) {
  const hash = createHash('sha256');
  hash.update(await readFile(filePath));
  return hash.digest('hex');
}

const entries = (await readdir(assetsDir)).sort();
const checksumLines = [];
const checksumByFilename = new Map();

for (const name of entries) {
  if (name === 'SHA256SUMS.txt') continue;
  const full = path.join(assetsDir, name);
  const info = await stat(full);
  if (!info.isFile()) continue;
  const digest = await sha256File(full);
  checksumByFilename.set(name, digest);
  checksumLines.push(`${digest}  ${name}`);
}

await writeFile(path.join(assetsDir, 'SHA256SUMS.txt'), `${checksumLines.join('\n')}\n`, 'utf8');
console.log(`Wrote SHA256SUMS.txt (${checksumLines.length} files)`);

const manifestAssets = {};
const missing = [];

for (const [targetKey, meta] of Object.entries(PORTABLE_TARGETS)) {
  const sha256 = checksumByFilename.get(meta.filename);
  if (!sha256) {
    missing.push(`${targetKey} (${meta.filename})`);
    continue;
  }
  manifestAssets[targetKey] = {
    filename: meta.filename,
    sha256,
    binaryRelativePath: meta.binaryRelativePath,
  };
}

if (missing.length > 0) {
  console.error(`Missing portable release archives for npm manifest:\n  - ${missing.join('\n  - ')}`);
  process.exit(1);
}

const manifest = {
  version,
  tag,
  repo,
  assets: manifestAssets,
};

const npmDir = path.join(root, 'packages', 'npm');
await writeFile(path.join(npmDir, 'release-manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`, 'utf8');

const npmPkgPath = path.join(npmDir, 'package.json');
const npmPkg = JSON.parse(await readFile(npmPkgPath, 'utf8'));
npmPkg.version = version;
await writeFile(npmPkgPath, `${JSON.stringify(npmPkg, null, 2)}\n`, 'utf8');

await copyFile(path.join(root, 'README.md'), path.join(npmDir, 'README.md'));
console.log(`Prepared packages/npm for termexo-fleet@${version} (${Object.keys(manifestAssets).length} targets)`);

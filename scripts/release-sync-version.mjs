#!/usr/bin/env node
/**
 * Synchronizes (or checks) the SemVer version across all workspace manifests
 * from a Git release tag such as `v0.2.0` or `v0.2.0-preview.1`.
 *
 * Usage:
 *   node scripts/release-sync-version.mjs v0.2.0
 *   node scripts/release-sync-version.mjs v0.2.0 --check
 */
import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const args = process.argv.slice(2);
const checkOnly = args.includes('--check');
const rawInput = args.find((a) => !a.startsWith('-'));

if (!rawInput) {
  console.error('Usage: node scripts/release-sync-version.mjs <tag-or-version> [--check]');
  process.exit(1);
}

const version = rawInput.replace(/^v/, '').trim();
if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(`Invalid SemVer version: "${rawInput}"`);
  process.exit(1);
}

const jsonTargets = [
  'package.json',
  'apps/desktop-ui/package.json',
  'src-tauri/tauri.conf.json',
  'packages/npm/package.json',
];

let mismatched = false;

for (const rel of jsonTargets) {
  const abs = path.join(root, rel);
  const raw = await readFile(abs, 'utf8');
  const parsed = JSON.parse(raw);
  if (parsed.version !== version) {
    if (checkOnly) {
      console.error(`Version mismatch in ${rel}: found "${parsed.version}", expected "${version}"`);
      mismatched = true;
    } else {
      parsed.version = version;
      await writeFile(abs, `${JSON.stringify(parsed, null, 2)}\n`, 'utf8');
      console.log(`Updated ${rel} -> ${version}`);
    }
  }
}

// Update [workspace.package] version in Cargo.toml
const cargoPath = path.join(root, 'Cargo.toml');
const cargoRaw = await readFile(cargoPath, 'utf8');
const workspaceVersionRe = /(\[workspace\.package\][\s\S]*?\r?\nversion\s*=\s*")([^"]+)(")/;
const match = cargoRaw.match(workspaceVersionRe);
if (!match) {
  console.error('Could not locate [workspace.package] version in Cargo.toml');
  process.exit(1);
}
if (match[2] !== version) {
  if (checkOnly) {
    console.error(`Version mismatch in Cargo.toml: found "${match[2]}", expected "${version}"`);
    mismatched = true;
  } else {
    const updated = cargoRaw.replace(workspaceVersionRe, `$1${version}$3`);
    await writeFile(cargoPath, updated, 'utf8');
    console.log(`Updated Cargo.toml -> ${version}`);
  }
}

if (mismatched) {
  process.exit(1);
}
console.log(checkOnly ? `All manifests match v${version}` : `Synced all manifests to ${version}`);

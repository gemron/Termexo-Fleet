import assert from 'node:assert/strict';
import http from 'node:http';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  buildDownloadUrl,
  ensureBinaryInstalled,
  resolveCacheDir,
  resolveTargetKey,
  verifyFileSha256,
} from '../lib/launcher.mjs';

test('resolveTargetKey maps supported OS/arch combinations', () => {
  assert.equal(resolveTargetKey('win32', 'x64'), 'windows-x64');
  assert.equal(resolveTargetKey('darwin', 'arm64'), 'macos-arm64');
  assert.equal(resolveTargetKey('darwin', 'x64'), 'macos-x64');
  assert.equal(resolveTargetKey('linux', 'x64'), 'linux-x64');
  assert.equal(resolveTargetKey('freebsd', 'x64'), null);
});

test('resolveCacheDir respects explicit env override and platform defaults', () => {
  const custom = resolveCacheDir('0.2.0', 'linux-x64', {
    env: { TERMEXO_FLEET_CACHE_DIR: '/tmp/custom-cache' },
    platform: 'linux',
    homedir: '/home/dev',
  });
  assert.equal(custom, path.join('/tmp/custom-cache', '0.2.0', 'linux-x64'));

  const mac = resolveCacheDir('0.2.0', 'macos-arm64', {
    env: {},
    platform: 'darwin',
    homedir: '/Users/dev',
  });
  assert.equal(mac, path.join('/Users/dev', 'Library', 'Caches', 'termexo-fleet', '0.2.0', 'macos-arm64'));
});

test('buildDownloadUrl uses GitHub Releases by default and supports mirror override', () => {
  const manifest = { version: '0.2.0', tag: 'v0.2.0', repo: 'gemron/Termexo-Fleet' };
  const asset = { filename: 'termexo-fleet-linux-x64.tar.gz' };
  assert.equal(
    buildDownloadUrl(manifest, asset, {}),
    'https://github.com/gemron/Termexo-Fleet/releases/download/v0.2.0/termexo-fleet-linux-x64.tar.gz',
  );
  assert.equal(
    buildDownloadUrl(manifest, asset, { TERMEXO_FLEET_DOWNLOAD_BASE_URL: 'https://mirror.local/dl/' }),
    'https://mirror.local/dl/termexo-fleet-linux-x64.tar.gz',
  );
});

test('verifyFileSha256 accepts matching digest and rejects tampered payload', async () => {
  const tmp = await mkdtemp(path.join(os.tmpdir(), 'fleet-npm-test-'));
  try {
    const file = path.join(tmp, 'sample.bin');
    const content = Buffer.from('termexo-fleet-binary-payload');
    await writeFile(file, content);
    const digest = createHash('sha256').update(content).digest('hex');

    assert.equal(await verifyFileSha256(file, digest), digest);
    await assert.rejects(() => verifyFileSha256(file, '0'.repeat(64)), /SHA-256 checksum mismatch/);
  } finally {
    await rm(tmp, { recursive: true, force: true });
  }
});

test('ensureBinaryInstalled downloads, verifies SHA-256, extracts, and caches archive', async () => {
  const tmp = await mkdtemp(path.join(os.tmpdir(), 'fleet-e2e-'));
  const cacheBase = path.join(tmp, 'cache');
  const srcDir = path.join(tmp, 'src');
  const archivePath = path.join(tmp, 'termexo-fleet-test.tar.gz');

  await writeFile(path.join(tmp, 'dummy.txt'), 'ok');
  const binName = process.platform === 'win32' ? 'termexo-fleet.exe' : 'termexo-fleet';

  const fsPromises = await import('node:fs/promises');
  await fsPromises.mkdir(srcDir, { recursive: true });
  await writeFile(path.join(srcDir, binName), '#!/bin/sh\necho ok\n');
  execFileSync('tar', ['-czf', archivePath, '-C', srcDir, binName]);

  const archiveBytes = await fsPromises.readFile(archivePath);
  const sha256 = createHash('sha256').update(archiveBytes).digest('hex');
  const targetKey = resolveTargetKey();
  assert.ok(targetKey, 'Current CI/dev platform should be supported');

  const server = http.createServer((req, res) => {
    if (req.url === '/termexo-fleet-test.tar.gz') {
      res.writeHead(200, { 'Content-Type': 'application/gzip' });
      res.end(archiveBytes);
      return;
    }
    res.writeHead(404);
    res.end();
  });

  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address();

  try {
    const manifest = {
      version: '0.1.0-test',
      tag: 'v0.1.0-test',
      repo: 'gemron/Termexo-Fleet',
      assets: {
        [targetKey]: {
          filename: 'termexo-fleet-test.tar.gz',
          sha256,
          binaryRelativePath: binName,
        },
      },
    };
    const env = {
      TERMEXO_FLEET_CACHE_DIR: cacheBase,
      TERMEXO_FLEET_DOWNLOAD_BASE_URL: `http://127.0.0.1:${port}`,
    };

    const first = await ensureBinaryInstalled({ manifest, env });
    assert.equal(first.downloaded, true);

    const second = await ensureBinaryInstalled({ manifest, env });
    assert.equal(second.downloaded, false);
    assert.equal(second.binaryPath, first.binaryPath);
  } finally {
    server.close();
    await rm(tmp, { recursive: true, force: true });
  }
});

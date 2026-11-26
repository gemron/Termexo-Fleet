import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import { executableCandidates, findExecutable } from '../lib/launcher.js';
import { checkLinuxRuntime, linuxCacheRoot, prepareLinuxApplication } from '../lib/linux.js';
import { prepareMacApplication } from '../lib/macos.js';

test('executableCandidates prioritizes TERMEXO_FLEET_PATH then bundled executable', () => {
  const candidates = executableCandidates(
    {
      TERMEXO_FLEET_PATH: 'C:\\custom\\termexo-fleet.exe',
      LOCALAPPDATA: 'C:\\Users\\dev\\AppData\\Local',
    },
    'C:\\pkg\\vendor\\win32-x64\\termexo-fleet.exe',
  );
  assert.equal(candidates[0], 'C:\\custom\\termexo-fleet.exe');
  assert.equal(candidates[1], 'C:\\pkg\\vendor\\win32-x64\\termexo-fleet.exe');
  assert.equal(
    findExecutable(
      { TERMEXO_FLEET_PATH: '' },
      (p) => p === 'C:\\pkg\\vendor\\win32-x64\\termexo-fleet.exe',
      'C:\\pkg\\vendor\\win32-x64\\termexo-fleet.exe',
    ),
    'C:\\pkg\\vendor\\win32-x64\\termexo-fleet.exe',
  );
});

test('prepareMacApplication extracts Termexo Fleet.app once and caches by digest', () => {
  const root = mkdtempSync(join(tmpdir(), 'fleet-mac-test-'));
  try {
    const src = join(root, 'src');
    const appContents = join(src, 'Termexo Fleet.app', 'Contents');
    mkdirSync(join(appContents, 'MacOS'), { recursive: true });
    writeFileSync(join(appContents, 'MacOS', 'termexo-fleet'), '#!/bin/sh\nexit 0\n');
    writeFileSync(join(appContents, 'Info.plist'), '<plist/>');

    const archive = join(root, 'Termexo Fleet.app.tar.gz');
    execFileSync('tar', ['-czf', archive, '-C', src, 'Termexo Fleet.app']);

    let extractCalls = 0;
    const cacheRoot = join(root, 'cache');
    const opts = {
      version: '0.1.0',
      arch: 'arm64',
      archivePath: archive,
      cacheRoot,
      extract: (arc, dst) => {
        extractCalls += 1;
        execFileSync('tar', ['-xzf', arc, '-C', dst]);
      },
    };
    const first = prepareMacApplication(opts);
    const second = prepareMacApplication(opts);
    assert.equal(first, second);
    assert.equal(extractCalls, 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('prepareLinuxApplication extracts usr/bin/termexo-fleet and checks runtime', () => {
  const root = mkdtempSync(join(tmpdir(), 'fleet-linux-test-'));
  try {
    assert.equal(
      linuxCacheRoot({ XDG_CACHE_HOME: '/tmp/xdg' }, '/home/dev'),
      join('/tmp/xdg', 'Termexo-Fleet', 'npm'),
    );

    const src = join(root, 'src');
    mkdirSync(join(src, 'usr', 'bin'), { recursive: true });
    const binFile = join(src, 'usr', 'bin', 'termexo-fleet');
    writeFileSync(binFile, '#!/bin/sh\nexit 0\n');
    chmodSync(binFile, 0o755);

    const archive = join(root, 'Termexo-Fleet.tar.gz');
    execFileSync('tar', ['-czf', archive, '-C', src, 'usr']);

    const prepared = prepareLinuxApplication({
      version: '0.1.0',
      arch: 'x64',
      archivePath: archive,
      cacheRoot: join(root, 'cache'),
    });
    assert.ok(prepared.endsWith(join('usr', 'bin', 'termexo-fleet')));

    assert.throws(
      () => checkLinuxRuntime(prepared, { environment: {} }),
      /DISPLAY or WAYLAND_DISPLAY/,
    );
    assert.doesNotThrow(() =>
      checkLinuxRuntime(prepared, {
        environment: { DISPLAY: ':0' },
        probe: () => ({ status: 0, stdout: 'libgtk-3.so.0 => /lib/libgtk-3.so.0', stderr: '' }),
      }),
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

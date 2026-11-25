import { createHash } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { createWriteStream, existsSync } from 'node:fs';
import { chmod, mkdir, readFile, rename, rm, stat } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
export const PACKAGE_ROOT = path.resolve(__dirname, '..');

export const DEFAULT_REPO = 'gemron/Termexo-Fleet';

export function resolveTargetKey(platform = process.platform, arch = process.arch) {
  if (platform === 'win32' && arch === 'x64') return 'windows-x64';
  if (platform === 'darwin' && arch === 'arm64') return 'macos-arm64';
  if (platform === 'darwin' && arch === 'x64') return 'macos-x64';
  if (platform === 'linux' && arch === 'x64') return 'linux-x64';
  return null;
}

export function resolveCacheDir(version, targetKey, options = {}) {
  const env = options.env ?? process.env;
  const platform = options.platform ?? process.platform;
  const home = options.homedir ?? os.homedir();

  if (env.TERMEXO_FLEET_CACHE_DIR) {
    return path.join(env.TERMEXO_FLEET_CACHE_DIR, version, targetKey);
  }
  if (platform === 'darwin') {
    return path.join(home, 'Library', 'Caches', 'termexo-fleet', version, targetKey);
  }
  if (platform === 'win32') {
    const base = env.LOCALAPPDATA || path.join(home, 'AppData', 'Local');
    return path.join(base, 'termexo-fleet', 'Cache', version, targetKey);
  }
  const xdg = env.XDG_CACHE_HOME || path.join(home, '.cache');
  return path.join(xdg, 'termexo-fleet', version, targetKey);
}

export async function loadManifest(manifestPath = path.join(PACKAGE_ROOT, 'release-manifest.json')) {
  const raw = await readFile(manifestPath, 'utf8');
  return JSON.parse(raw);
}

export function buildDownloadUrl(manifest, asset, env = process.env) {
  const baseOverride = env.TERMEXO_FLEET_DOWNLOAD_BASE_URL?.replace(/\/+$/, '');
  if (baseOverride) {
    return `${baseOverride}/${asset.filename}`;
  }
  const repo = manifest.repo || DEFAULT_REPO;
  const tag = manifest.tag || `v${manifest.version}`;
  return `https://github.com/${repo}/releases/download/${tag}/${asset.filename}`;
}

export async function computeFileSha256(filePath) {
  const hash = createHash('sha256');
  const data = await readFile(filePath);
  hash.update(data);
  return hash.digest('hex');
}

export async function verifyFileSha256(filePath, expectedSha256) {
  if (!expectedSha256) {
    throw new Error('Release manifest is missing expected SHA-256 checksum.');
  }
  const actual = await computeFileSha256(filePath);
  if (actual.toLowerCase() !== expectedSha256.toLowerCase()) {
    throw new Error(
      `SHA-256 checksum mismatch for ${path.basename(filePath)}:\n  expected: ${expectedSha256}\n  actual:   ${actual}`,
    );
  }
  return actual;
}

async function downloadFile(url, destPath) {
  const response = await fetch(url, {
    redirect: 'follow',
    headers: { 'User-Agent': 'termexo-fleet-npm-launcher' },
  });
  if (!response.ok || !response.body) {
    throw new Error(`Failed to download ${url} (HTTP ${response.status} ${response.statusText})`);
  }
  const fileStream = createWriteStream(destPath);
  await pipeline(Readable.fromWeb(response.body), fileStream);
}

function extractArchive(archivePath, destDir, platform = process.platform) {
  if (archivePath.endsWith('.tar.gz') || archivePath.endsWith('.tgz')) {
    const res = spawnSync('tar', ['-xzf', archivePath, '-C', destDir], { stdio: 'inherit' });
    if (res.status !== 0) {
      throw new Error(`tar extraction failed with exit code ${res.status ?? 'unknown'}`);
    }
    return;
  }

  if (archivePath.endsWith('.zip')) {
    // Windows 10/11 includes bsdtar; fall back to PowerShell Expand-Archive if unavailable.
    const tarAttempt = spawnSync('tar', ['-xf', archivePath, '-C', destDir], { stdio: 'pipe' });
    if (tarAttempt.status === 0) return;

    if (platform === 'win32') {
      const escapedSrc = archivePath.replace(/'/g, "''");
      const escapedDst = destDir.replace(/'/g, "''");
      const ps = spawnSync(
        'powershell.exe',
        [
          '-NoProfile',
          '-NonInteractive',
          '-Command',
          `Expand-Archive -LiteralPath '${escapedSrc}' -DestinationPath '${escapedDst}' -Force`,
        ],
        { stdio: 'inherit' },
      );
      if (ps.status !== 0) {
        throw new Error(`Expand-Archive failed with exit code ${ps.status ?? 'unknown'}`);
      }
      return;
    }

    const unzip = spawnSync('unzip', ['-qo', archivePath, '-d', destDir], { stdio: 'inherit' });
    if (unzip.status !== 0) {
      throw new Error(`unzip failed with exit code ${unzip.status ?? 'unknown'}`);
    }
    return;
  }

  throw new Error(`Unsupported archive format: ${archivePath}`);
}

export async function ensureBinaryInstalled(options = {}) {
  const platform = options.platform ?? process.platform;
  const arch = options.arch ?? process.arch;
  const env = options.env ?? process.env;
  const forceDownload = Boolean(options.forceDownload);
  const manifest = options.manifest ?? (await loadManifest());

  const targetKey = resolveTargetKey(platform, arch);
  if (!targetKey) {
    throw new Error(
      `Unsupported platform ${platform}/${arch}. Termexo Fleet supports Windows (x64), macOS (arm64, x64), and Linux (x64).`,
    );
  }

  const asset = manifest.assets?.[targetKey];
  if (!asset || !asset.filename || !asset.binaryRelativePath) {
    throw new Error(
      `Release v${manifest.version} does not contain a prebuilt binary entry for ${targetKey}.`,
    );
  }

  const cacheDir = resolveCacheDir(manifest.version, targetKey, { env, platform });
  const binaryPath = path.join(cacheDir, asset.binaryRelativePath);

  if (!forceDownload && existsSync(binaryPath)) {
    return { binaryPath, cacheDir, targetKey, downloaded: false };
  }

  const parentDir = path.dirname(cacheDir);
  await mkdir(parentDir, { recursive: true });
  const stagingDir = path.join(parentDir, `.staging-${targetKey}-${Date.now()}`);
  const archivePath = path.join(parentDir, `.download-${Date.now()}-${asset.filename}`);

  const url = buildDownloadUrl(manifest, asset, env);
  console.error(`[termexo-fleet] Downloading v${manifest.version} (${targetKey})...`);
  console.error(`[termexo-fleet] ${url}`);

  try {
    await downloadFile(url, archivePath);
    await verifyFileSha256(archivePath, asset.sha256);

    await mkdir(stagingDir, { recursive: true });
    extractArchive(archivePath, stagingDir, platform);

    const stagedBinary = path.join(stagingDir, asset.binaryRelativePath);
    if (!existsSync(stagedBinary)) {
      throw new Error(
        `Archive extracted successfully, but expected binary was not found at ${asset.binaryRelativePath}`,
      );
    }

    if (platform !== 'win32') {
      await chmod(stagedBinary, 0o755);
    }

    await rm(cacheDir, { recursive: true, force: true });
    await rename(stagingDir, cacheDir);
  } catch (err) {
    await rm(stagingDir, { recursive: true, force: true }).catch(() => {});
    throw err;
  } finally {
    await rm(archivePath, { force: true }).catch(() => {});
  }

  await stat(binaryPath);
  console.error(`[termexo-fleet] Ready at ${binaryPath}`);
  return { binaryPath, cacheDir, targetKey, downloaded: true };
}

export function printHelp(version) {
  console.log(`Termexo Fleet v${version} — Local-first AI coding fleet commander

Usage:
  npx termexo-fleet@latest [options]
  termexo-fleet [options]

Options:
  -v, --version         Print version
  -h, --help            Show this help message
  --install-only        Download and verify the desktop binary without launching
  --force-download      Re-download and re-verify the cached desktop binary
  --foreground          Keep attached to stdout/stderr until the app exits
  --cache-dir           Print the local binary cache path for this platform

Environment:
  TERMEXO_FLEET_CACHE_DIR           Override local binary cache directory
  TERMEXO_FLEET_DOWNLOAD_BASE_URL   Override release download base URL (mirror)`);
}

export async function runCli(argv = process.argv.slice(2)) {
  const pkg = JSON.parse(await readFile(path.join(PACKAGE_ROOT, 'package.json'), 'utf8'));

  if (argv.includes('-v') || argv.includes('--version')) {
    console.log(pkg.version);
    return 0;
  }
  if (argv.includes('-h') || argv.includes('--help')) {
    printHelp(pkg.version);
    return 0;
  }

  const targetKey = resolveTargetKey();
  if (argv.includes('--cache-dir')) {
    console.log(targetKey ? resolveCacheDir(pkg.version, targetKey) : 'unsupported-platform');
    return 0;
  }

  const installOnly = argv.includes('--install-only');
  const forceDownload = argv.includes('--force-download');
  const foreground = argv.includes('--foreground');
  const passthroughArgs = argv.filter(
    (arg) => !['--install-only', '--force-download', '--foreground'].includes(arg),
  );

  const { binaryPath } = await ensureBinaryInstalled({ forceDownload });
  if (installOnly) {
    return 0;
  }

  if (foreground) {
    return await new Promise((resolve) => {
      const child = spawn(binaryPath, passthroughArgs, { stdio: 'inherit' });
      child.on('error', (err) => {
        console.error(`[termexo-fleet] Failed to launch binary: ${err.message}`);
        if (process.platform === 'linux') {
          console.error(
            '[termexo-fleet] Linux requires WebKitGTK/GTK runtime libraries, e.g.: sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1',
          );
        }
        resolve(1);
      });
      child.on('exit', (code) => resolve(code ?? 0));
    });
  }

  const child = spawn(binaryPath, passthroughArgs, {
    detached: true,
    stdio: 'ignore',
  });

  return await new Promise((resolve) => {
    child.once('error', (err) => {
      console.error(`[termexo-fleet] Failed to launch Termexo Fleet: ${err.message}`);
      if (process.platform === 'linux') {
        console.error(
          '[termexo-fleet] On Debian/Ubuntu, install runtime libs: sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1',
        );
      }
      resolve(1);
    });
    child.once('spawn', () => {
      child.unref();
      resolve(0);
    });
  });
}

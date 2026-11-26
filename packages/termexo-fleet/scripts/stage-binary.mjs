import { copyFile, mkdir, open, stat } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repositoryRoot = resolve(packageRoot, '..', '..');
const destinationDirectory = resolve(packageRoot, 'vendor', 'win32-x64');
const targetDirectory = process.env.CARGO_TARGET_DIR
  ? resolve(repositoryRoot, process.env.CARGO_TARGET_DIR)
  : resolve(repositoryRoot, 'target');

const MINIMUM_EXECUTABLE_BYTES = 1_000_000;
const source = resolve(targetDirectory, 'release', 'termexo-fleet.exe');

async function assertWindowsBinary(filePath) {
  const handle = await open(filePath, 'r');
  try {
    const signature = Buffer.alloc(2);
    await handle.read(signature, 0, signature.length, 0);
    if (signature.toString('ascii') !== 'MZ') {
      throw new Error(`${filePath} is not a valid Windows binary.`);
    }
  } finally {
    await handle.close();
  }
}

const info = await stat(source).catch(() => null);
if (!info?.isFile() || info.size < MINIMUM_EXECUTABLE_BYTES) {
  throw new Error(
    `A current release executable was not found at ${source}. Run npm run tauri:build first.`,
  );
}
await assertWindowsBinary(source);
await mkdir(destinationDirectory, { recursive: true });
await copyFile(source, resolve(destinationDirectory, 'termexo-fleet.exe'));
console.log(`Staged Windows desktop binary for npm packaging (${info.size} bytes).`);

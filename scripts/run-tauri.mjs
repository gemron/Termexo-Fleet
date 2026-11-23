import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const args = process.argv.slice(2);

if (process.platform === 'win32') {
  const result = spawnSync('cmd.exe', ['/d', '/c', join(root, 'scripts', 'tauri-msvc.cmd'), ...args], {
    cwd: root,
    stdio: 'inherit',
  });
  process.exit(result.status ?? 1);
} else {
  const tauriBin = join(root, 'apps', 'desktop-ui', 'node_modules', '.bin', 'tauri');
  const result = spawnSync(tauriBin, args, { cwd: root, stdio: 'inherit' });
  process.exit(result.status ?? 1);
}

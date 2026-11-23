import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { resolve } from 'node:path';

const binary = process.platform === 'win32' ? 'node.exe' : 'node';
const candidates = [
  resolve('node_modules', 'node', 'bin', binary),
  resolve('..', '..', '..', 'Termexo', 'apps', 'desktop-ui', 'node_modules', 'node', 'bin', binary),
  process.execPath,
];
const projectNode = candidates.find((candidate) => existsSync(candidate)) ?? process.execPath;
const angularCli = resolve('node_modules', '@angular', 'cli', 'bin', 'ng.js');

const result = spawnSync(projectNode, [angularCli, ...process.argv.slice(2)], {
  stdio: 'inherit',
});

if (result.error) {
  console.error(`Unable to start the project Node runtime: ${result.error.message}`);
  process.exit(1);
}

process.exit(result.status ?? 1);

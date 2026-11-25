#!/usr/bin/env node
import { runCli } from '../lib/launcher.mjs';

try {
  const code = await runCli(process.argv.slice(2));
  process.exit(code);
} catch (error) {
  console.error(`[termexo-fleet] ${error instanceof Error ? error.message : String(error)}`);
  process.exit(1);
}

#!/usr/bin/env node

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import {
  RELEASES_URL,
  findExecutable,
  launchExecutable,
  openDownloadPage,
} from '../lib/launcher.js';
import { checkLinuxRuntime, prepareLinuxApplication } from '../lib/linux.js';
import { prepareMacApplication } from '../lib/macos.js';
import { launchNativeApplication, openNativeDownloadPage } from '../lib/native-launch.js';

const packageJsonPath = fileURLToPath(new URL('../package.json', import.meta.url));
const packageJson = JSON.parse(readFileSync(packageJsonPath, 'utf8'));
const command = process.argv[2]?.toLowerCase() ?? 'start';

function printHelp() {
  console.log(`Termexo Fleet ${packageJson.version}

Usage:
  termexo-fleet              Start the bundled Termexo Fleet desktop app
  termexo-fleet start        Start the bundled Termexo Fleet desktop app
  termexo-fleet download     Open the latest Termexo Fleet release page
  termexo-fleet --version    Print the launcher version
  termexo-fleet --help       Show this help

Set TERMEXO_FLEET_PATH to the full path of termexo-fleet.exe, Termexo Fleet.app or the Linux termexo-fleet executable when using a custom installation directory.`);
}

if (command === '--help' || command === '-h' || command === 'help') {
  printHelp();
} else if (command === '--version' || command === '-v' || command === 'version') {
  console.log(packageJson.version);
} else if (command === 'download') {
  try {
    if (process.platform === 'linux' || process.platform === 'darwin') {
      await openNativeDownloadPage(RELEASES_URL);
    } else {
      openDownloadPage();
    }
    console.log(`Opened ${RELEASES_URL}`);
  } catch (error) {
    console.error(
      `Could not open the release page: ${error.message}. On Linux, install xdg-utils.`,
    );
    process.exitCode = 1;
  }
} else if (command === 'start') {
  try {
    const executable =
      process.platform === 'darwin'
        ? process.env.TERMEXO_FLEET_PATH ||
          prepareMacApplication({ version: packageJson.version })
        : process.platform === 'linux'
          ? process.env.TERMEXO_FLEET_PATH ||
            prepareLinuxApplication({ version: packageJson.version })
          : findExecutable();
    if (!executable) {
      throw new Error(`The Termexo Fleet executable is missing from this npm package.

Reinstall the package or download the installer for your platform:
  npm install --global termexo-fleet@latest
  termexo-fleet download
  ${RELEASES_URL}

For a custom installation, set TERMEXO_FLEET_PATH to termexo-fleet.exe, Termexo Fleet.app or the Linux executable.`);
    }
    if (process.platform === 'linux') checkLinuxRuntime(executable);
    if (process.platform === 'linux' || process.platform === 'darwin') {
      await launchNativeApplication(executable);
    } else {
      launchExecutable(executable);
    }
    console.log(`Started Termexo Fleet from ${executable}`);
  } catch (error) {
    console.error(`Could not start Termexo Fleet: ${error.message}`);
    process.exitCode = 1;
  }
} else {
  console.error(`Unknown command: ${process.argv[2]}`);
  printHelp();
  process.exitCode = 1;
}

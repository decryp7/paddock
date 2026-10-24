// Builds the app's sample agent (src-tauri/agent, plan X4) and puts it where Tauri's `externalBin` expects it:
// src-tauri/binaries/shell-demo-agent-<target triple>[.exe]. Run by `beforeBuildCommand` (release profile) and
// `beforeDevCommand` (`--dev`, the debug profile). Honours CARGO_TARGET_DIR, TAURI_ENV_TARGET_TRIPLE and
// PADDOCK_CARGO_PROFILE. The mechanism is the family's (`paddock-sidecar`, plan X34): this file only names the sidecar.
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { main } from '../../../../conformance/src/sidecar/sidecar.mjs';

const app = resolve(dirname(fileURLToPath(import.meta.url)), '..');
process.exitCode = main(['build', '--root', app, '--out', join(app, 'binaries'), '--manifest-path', join(app, 'Cargo.toml'), '--sidecar', 'shell-demo-agent', ...process.argv.slice(2)]);

// Builds the app's sample agent (src-tauri/agent, plan X4) and puts it where Tauri's `externalBin` expects it:
// src-tauri/binaries/shell-demo-agent-<target triple>[.exe]. Run by `beforeBuildCommand` (release profile) and
// `beforeDevCommand` (`--dev`, the debug profile). Honours
// CARGO_TARGET_DIR and TAURI_ENV_TARGET_TRIPLE (a cross build). Adapted from Pitwall's and Pitcrew's scripts/sidecar.mjs.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const app = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const dev = process.argv.includes('--dev');
const host = /host: (\S+)/.exec(execFileSync('rustc', ['-vV'], { encoding: 'utf8' }))?.[1];
const triple = process.env.TAURI_ENV_TARGET_TRIPLE || host;
if (!triple) throw new Error('cannot tell the target triple (rustc -vV)');
const args = ['build', '--manifest-path', join(app, 'Cargo.toml'), '-p', 'shell-demo-agent', '--bin', 'shell-demo-agent'];
if (!dev) args.push('--release');
if (triple !== host) args.push('--target', triple);
execFileSync('cargo', args, { cwd: app, stdio: 'inherit' });
const targetDir = process.env.CARGO_TARGET_DIR ? resolve(process.env.CARGO_TARGET_DIR) : join(app, 'target');
const exe = triple.includes('windows') ? '.exe' : '';
const built = join(targetDir, ...(triple !== host ? [triple] : []), dev ? 'debug' : 'release', `shell-demo-agent${exe}`);
if (statSync(built).size === 0) throw new Error(`${built} is empty`);
const out = join(app, 'binaries', `shell-demo-agent-${triple}${exe}`);
mkdirSync(dirname(out), { recursive: true });
copyFileSync(built, out);
console.log(`sidecar: ${out}`);

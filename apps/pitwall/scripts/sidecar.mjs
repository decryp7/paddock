// Builds pitwall-server and puts it where Tauri's `externalBin` expects it:
// app/src-tauri/binaries/pitwall-server-<target triple>[.exe]. Run by `beforeBuildCommand` / `beforeDevCommand`
// (tauri.conf.json); `--dev` builds the debug profile. Honours CARGO_TARGET_DIR, a build.target-dir in the cargo configuration (CI's .cargo/ci.toml) and TAURI_ENV_TARGET_TRIPLE.
// (Adapted from Pitcrew's scripts/sidecar.mjs.)
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { cargoTargetDir } from '../../../conformance/src/sidecar/sidecar.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const dev = process.argv.includes('--dev');
const host = /host: (\S+)/.exec(execFileSync('rustc', ['-vV'], { encoding: 'utf8' }))?.[1];
const triple = process.env.TAURI_ENV_TARGET_TRIPLE || host;
if (!triple) throw new Error('cannot tell the target triple (rustc -vV)');
const profile = dev ? 'dev' : 'dist';
const args = ['build', '-p', 'pitwall-server', '--bin', 'pitwall-server', '--profile', profile];
if (triple !== host) args.push('--target', triple);
execFileSync('cargo', args, { cwd: root, stdio: 'inherit' });
const targetDir = cargoTargetDir(root);
const exe = triple.includes('windows') ? '.exe' : '';
const built = join(targetDir, ...(triple !== host ? [triple] : []), dev ? 'debug' : 'dist', `pitwall-server${exe}`);
const out = join(root, 'app', 'src-tauri', 'binaries', `pitwall-server-${triple}${exe}`);
mkdirSync(dirname(out), { recursive: true });
copyFileSync(built, out);
console.log(`sidecar: ${out}`);

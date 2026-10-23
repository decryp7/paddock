// Builds dyno-agent, dyno-helper and the dyno command (crate dyno-cli) and puts them where Tauri's `externalBin` expects them:
// app/src-tauri/binaries/<name>-<target triple>[.exe]. Run by `beforeBuildCommand` / `beforeDevCommand`
// (tauri.conf.json); `--dev` builds the debug profile. Honours CARGO_TARGET_DIR and TAURI_ENV_TARGET_TRIPLE.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const dev = process.argv.includes('--dev');
const host = /host: (\S+)/.exec(execFileSync('rustc', ['-vV'], { encoding: 'utf8' }))?.[1];
const triple = process.env.TAURI_ENV_TARGET_TRIPLE || host;
if (!triple) throw new Error('cannot tell the target triple (rustc -vV)');
const profile = dev ? 'dev' : 'dist';
// [binary name, package]: the CLI's binary is `dyno`, its package `dyno-cli`.
const sidecars = [['dyno-agent', 'dyno-agent'], ['dyno-helper', 'dyno-helper'], ['dyno', 'dyno-cli']];
const args = ['build', ...sidecars.flatMap(([, pkg]) => ['-p', pkg]), '--profile', profile];
if (triple !== host) args.push('--target', triple);
execFileSync('cargo', args, { cwd: root, stdio: 'inherit' });
const targetDir = process.env.CARGO_TARGET_DIR ? resolve(process.env.CARGO_TARGET_DIR) : join(root, 'target');
const exe = triple.includes('windows') ? '.exe' : '';
for (const [name] of sidecars) {
  const built = join(targetDir, ...(triple !== host ? [triple] : []), dev ? 'debug' : 'dist', `${name}${exe}`);
  const out = join(root, 'app', 'src-tauri', 'binaries', `${name}-${triple}${exe}`);
  mkdirSync(dirname(out), { recursive: true });
  copyFileSync(built, out);
  console.log(`sidecar: ${out}`);
}

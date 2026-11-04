// Builds dyno-agent, the dyno command (crate dyno-cli) and the benchmark runner dyno-bench (ADR 0030) and puts them where Tauri's `externalBin` expects them:
// app/src-tauri/binaries/<name>-<target triple>[.exe]. Run by `beforeBuildCommand` / `beforeDevCommand`
// (tauri.conf.json); `--dev` builds the debug profile. Honours CARGO_TARGET_DIR, a build.target-dir in the cargo configuration (CI's .cargo/ci.toml) and TAURI_ENV_TARGET_TRIPLE.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { cargoTargetDir } from '../../../../../conformance/src/sidecar/sidecar.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const dev = process.argv.includes('--dev');
const host = /host: (\S+)/.exec(execFileSync('rustc', ['-vV'], { encoding: 'utf8' }))?.[1];
const triple = process.env.TAURI_ENV_TARGET_TRIPLE || host;
if (!triple) throw new Error('cannot tell the target triple (rustc -vV)');
const profile = dev ? 'dev' : 'dist';
// [binary name, package]: the CLI's binary is `dyno`, its package `dyno-cli`. The helper (ADR 0027) ships next to the agent on every
// OS: the program the one OS prompt runs to install it (macOS, Windows), `/usr/bin/dyno-helper` in the .deb; the AppImage never uses it.
const sidecars = [['dyno-agent', 'dyno-agent'], ['dyno', 'dyno-cli'], ['dyno-bench', 'dyno-bench'], ['dyno-bench-gpu', 'dyno-bench-gpu'], ['dyno-helper', 'dyno-helper']];
const args = ['build', ...sidecars.flatMap(([, pkg]) => ['-p', pkg]), '--profile', profile];
if (triple !== host) args.push('--target', triple);
execFileSync('cargo', args, { cwd: root, stdio: 'inherit' });
const targetDir = cargoTargetDir(root);
const exe = triple.includes('windows') ? '.exe' : '';
for (const [name] of sidecars) {
  const built = join(targetDir, ...(triple !== host ? [triple] : []), dev ? 'debug' : 'dist', `${name}${exe}`);
  const out = join(root, 'app', 'src-tauri', 'binaries', `${name}-${triple}${exe}`);
  mkdirSync(dirname(out), { recursive: true });
  copyFileSync(built, out);
  console.log(`sidecar: ${out}`);
}

// Security review F2 (ADR 0027 amendment): on macOS the helper that the one password prompt copies into /Library as root sits in a
// user-owned bundle, so the release signs its final bytes with the family update key. Tauri re-signs every sidecar ad hoc
// (tauri-bundler macos/app.rs: `codesign --force -s <identity>` on every sidecar copy, plus `--options runtime` only with the hardened
// runtime, which tauri-bundle.sh turns on only with a real Apple identity), so the helper is sealed here the same way first. Measured
// 2026-11-04: an ad-hoc `codesign --force -s -` of an already ad-hoc signed dyno-helper under another file name leaves the bytes
// identical (the identifier comes from the embedded Info.plist), so Tauri ships the bytes signed here. With a Developer ID the
// bytes would change: check-bundle.sh then fails loudly, and that release signs after the bundle instead.
// The signature goes to binaries/dyno-helper.minisig (bundled into Contents/Resources by tauri.macos.conf.json) and the hash it
// covers to binaries/dyno-helper.sha256, which scripts/check-bundle.sh compares with the helper inside the built .app (a mismatch
// fails the release instead of shipping a helper the install would refuse). Without the signing key (forks, pull requests, dev)
// the signature file is empty: the elevated install then refuses with "no signature file", it never installs unchecked.
if (triple.includes('apple-darwin')) {
  const helper = join(root, 'app', 'src-tauri', 'binaries', `dyno-helper-${triple}`);
  const sig = join(root, 'app', 'src-tauri', 'binaries', 'dyno-helper.minisig');
  const hashFile = join(root, 'app', 'src-tauri', 'binaries', 'dyno-helper.sha256');
  if (process.env.TAURI_SIGNING_PRIVATE_KEY) {
    execFileSync('codesign', ['--force', '-s', '-', helper], { stdio: 'inherit' });
    execFileSync('npx', ['tauri', 'signer', 'sign', helper], { cwd: join(root, 'app'), stdio: 'inherit' });
    copyFileSync(`${helper}.sig`, sig);
    writeFileSync(hashFile, createHash('sha256').update(readFileSync(helper)).digest('hex') + '\n');
    console.log(`helper signature: ${sig} covers sha256 ${readFileSync(hashFile, 'utf8').trim()}`);
  } else {
    writeFileSync(sig, '');
    writeFileSync(hashFile, '');
    console.log('::warning::no TAURI_SIGNING_PRIVATE_KEY: the bundled helper carries no signature, its install will be refused');
  }
}

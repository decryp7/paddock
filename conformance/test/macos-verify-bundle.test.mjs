// Tests for the shared macos-verify-bundle.sh (moved from Pitcrew's copy, rule 43) on small throwaway .app bundles built in a temp folder (macOS only: it needs
// codesign and a C compiler). Nothing outside the temp folder is touched; every bundle is signed ad hoc.
import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const script = join(here, '..', 'release', 'macos-verify-bundle.sh');
const hasCc = process.platform === 'darwin' && spawnSync('cc', ['--version']).status === 0;
const skip = hasCc ? false : 'needs macOS with codesign and a C compiler';

const work = hasCc ? mkdtempSync(join(tmpdir(), 'verify-bundle-')) : '';
test.after(() => work && rmSync(work, { recursive: true, force: true }));

/** Compiles a program that exits with `code` and returns its path. */
function program(name, code) {
  const out = join(work, `${name}-${code}`);
  execFileSync('cc', ['-x', 'c', '-', '-o', out], { input: `int main(void) { return ${code}; }\n` });
  return out;
}

let n = 0;
/** A program that would run for a minute (a sidecar that hangs on a lock). */
function hangingProgram() {
  const out = join(work, 'hang');
  execFileSync('cc', ['-x', 'c', '-', '-o', out], { input: '#include <unistd.h>\nint main(void) { sleep(60); return 0; }\n' });
  return out;
}

/** Builds and signs a .app with a main executable and one sidecar. */
function bundle({ hardened = false, sidecarExit = 0, sidecar = 'fake-sidecar', hang = false } = {}) {
  const app = join(work, `b${n++}`, 'Fake.app');
  const macos = join(app, 'Contents', 'MacOS');
  mkdirSync(macos, { recursive: true });
  writeFileSync(join(app, 'Contents', 'Info.plist'), `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>fake-app</string>
<key>CFBundleIdentifier</key><string>net.example.fake</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
`);
  execFileSync('cp', [program('app', 0), join(macos, 'fake-app')]);
  execFileSync('cp', [hang ? hangingProgram() : program('sidecar', sidecarExit), join(macos, sidecar)]);
  const opts = hardened ? ['--options', 'runtime'] : [];
  execFileSync('codesign', ['--force', '-s', '-', ...opts, join(macos, sidecar)], { stdio: 'pipe' });
  execFileSync('codesign', ['--force', '-s', '-', ...opts, app], { stdio: 'pipe' });
  return app;
}

/** A `codesign` on PATH that reports a vendor Team ID for the sidecar, as a library signed by its vendor would. */
function vendorTeamShim() {
  const bin = join(work, 'shim');
  mkdirSync(bin, { recursive: true });
  const shim = join(bin, 'codesign');
  writeFileSync(shim, `#!/bin/bash
out=$(/usr/bin/codesign "$@" 2>&1); rc=$?
case "$*" in *fake-sidecar*|*a-sidecar*) out=$(printf '%s\\n' "$out" | sed 's/^TeamIdentifier=.*/TeamIdentifier=VENDOR1234/') ;; esac
printf '%s\\n' "$out" >&2
exit $rc
`);
  chmodSync(shim, 0o755);
  return bin;
}

function verify(app, { args = ['--sidecar', 'fake-sidecar:--version'], env = {} } = {}) {
  const r = spawnSync('bash', [script, ...args, app], { encoding: 'utf8', env: { ...process.env, CARGO_TARGET_DIR: join(work, 'no-target'), ...env } });
  return { status: r.status, out: `${r.stdout}${r.stderr}` };
}

test('an ad-hoc bundle without the hardened runtime whose sidecar starts passes', { skip }, () => {
  const r = verify(bundle());
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /started: fake-sidecar --version/);
  assert.match(r.out, /bundle OK/);
});

test('an ad-hoc bundle with the hardened runtime on fails', { skip }, () => {
  const r = verify(bundle({ hardened: true }));
  assert.notEqual(r.status, 0, r.out);
  assert.match(r.out, /hardened runtime on: set bundle\.macOS\.hardenedRuntime to false/);
});

test('a bundled Mach-O with another TeamIdentifier fails', { skip }, () => {
  const r = verify(bundle(), { env: { PATH: `${vendorTeamShim()}:${process.env.PATH}` } });
  assert.notEqual(r.status, 0, r.out);
  assert.match(r.out, /fake-sidecar has Team ID 'VENDOR1234', the app has ''/);
});

test('a sidecar that does not start from the built app fails', { skip }, () => {
  const r = verify(bundle({ sidecarExit: 3 }));
  assert.notEqual(r.status, 0, r.out);
  assert.match(r.out, /sidecar fake-sidecar of .* does not start/);
});

test('a sidecar named but missing from the app fails; with no --sidecar nothing is started', { skip }, () => {
  const missing = verify(bundle(), { args: ['--sidecar', 'not-there'] });
  assert.notEqual(missing.status, 0, missing.out);
  assert.match(missing.out, /sidecar not-there is missing or not executable/);
  const none = verify(bundle(), { args: [] });
  assert.equal(none.status, 0, none.out);
});

test('the default sidecar argument is --self-test', { skip }, () => {
  const r = verify(bundle(), { args: ['--sidecar', 'fake-sidecar'] });
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /started: fake-sidecar --self-test/);
});

test('the bundle folder comes from --bundle-dir, else the build-folder action, else cargo', { skip }, () => {
  const app = bundle();
  const folder = join(work, 'bf');
  mkdirSync(join(folder, 'dist', 'bundle', 'macos'), { recursive: true });
  execFileSync('cp', ['-R', app, join(folder, 'dist', 'bundle', 'macos', 'Fake.app')]);
  const viaFolder = spawnSync('bash', [script], { encoding: 'utf8', env: { ...process.env, PADDOCK_BUILD_FOLDER: folder, CARGO_TARGET_DIR: join(work, 'no-target') } });
  assert.equal(viaFolder.status, 0, `${viaFolder.stdout}${viaFolder.stderr}`);
  const viaFlag = spawnSync('bash', [script, '--bundle-dir', join(folder, 'dist', 'bundle')], { encoding: 'utf8', env: { ...process.env, CARGO_TARGET_DIR: join(work, 'no-target') } });
  assert.equal(viaFlag.status, 0, `${viaFlag.stdout}${viaFlag.stderr}`);
});

test('a missing bundle fails with a message', { skip }, () => {
  const r = verify(join(work, 'nothing', 'None.app'));
  assert.notEqual(r.status, 0);
  assert.match(r.out, /no \.app bundle found/);
});

test('a relative bundle path is resolved before the script changes folder', { skip }, () => {
  const app = bundle();
  const r = spawnSync('bash', [script, '--sidecar', 'fake-sidecar:--version', 'Fake.app'], {
    cwd: dirname(app),
    encoding: 'utf8',
    env: { ...process.env, CARGO_TARGET_DIR: join(work, 'no-target') },
  });
  assert.equal(r.status, 0, `${r.stdout}${r.stderr}`);
});

test('the main executable is the one Info.plist names, even when a sidecar sorts before it', { skip }, () => {
  // a-sidecar is found first by `find`/glob order; judged against it, the real main executable would be the odd one out.
  const r = verify(bundle({ sidecar: 'a-sidecar' }), { args: ['--sidecar', 'a-sidecar:--version'], env: { PATH: `${vendorTeamShim()}:${process.env.PATH}` } });
  assert.notEqual(r.status, 0, r.out);
  assert.match(r.out, /a-sidecar has Team ID 'VENDOR1234', the app has ''/);
  assert.equal(verify(bundle({ sidecar: 'a-sidecar' }), { args: ['--sidecar', 'a-sidecar:--version'] }).status, 0);
});

test('a sidecar that hangs is stopped after SIDECAR_TIMEOUT and fails the bundle', { skip }, () => {
  const started = Date.now();
  const r = verify(bundle({ hang: true }), { env: { SIDECAR_TIMEOUT: '1' } });
  assert.notEqual(r.status, 0, r.out);
  assert.match(r.out, /sidecar fake-sidecar of .* does not start/);
  assert.ok(Date.now() - started < 30000, 'the 60-second sleep was cut short');
});

test('--tauri-conf starts every bundle.externalBin when no --sidecar is given; an explicit --sidecar wins', { skip }, () => {
  const conf = join(work, 'tauri.conf.json');
  writeFileSync(conf, JSON.stringify({ bundle: { externalBin: ['binaries/fake-sidecar', 'binaries/not-there'] } }));
  const missing = verify(bundle(), { args: ['--tauri-conf', conf] });
  assert.notEqual(missing.status, 0, missing.out);
  assert.match(missing.out, /started: fake-sidecar --self-test/);
  assert.match(missing.out, /sidecar not-there is missing or not executable/);
  const explicit = verify(bundle(), { args: ['--tauri-conf', conf, '--sidecar', 'fake-sidecar:--version'] });
  assert.equal(explicit.status, 0, explicit.out);
  assert.equal(verify(bundle(), { args: ['--tauri-conf', join(work, 'nope.json')] }).status, 2);
});

// Tests for macos-verify-bundle.sh on small throwaway .app bundles built in a temp folder (macOS only: it needs
// codesign and a C compiler). Nothing outside the temp folder is touched; every bundle is signed ad hoc.
import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const script = join(here, 'macos-verify-bundle.sh');
const root = join(here, '..', '..');
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
/** Builds and signs a .app with a main executable and one sidecar. */
function bundle({ hardened = false, sidecarExit = 0 } = {}) {
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
  execFileSync('cp', [program('sidecar', sidecarExit), join(macos, 'fake-sidecar')]);
  const opts = hardened ? ['--options', 'runtime'] : [];
  execFileSync('codesign', ['--force', '-s', '-', ...opts, join(macos, 'fake-sidecar')], { stdio: 'pipe' });
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
case "$*" in *fake-sidecar*) out=$(printf '%s\\n' "$out" | sed 's/^TeamIdentifier=.*/TeamIdentifier=VENDOR1234/') ;; esac
printf '%s\\n' "$out" >&2
exit $rc
`);
  chmodSync(shim, 0o755);
  return bin;
}

function verify(app, env = {}) {
  const r = spawnSync('bash', [script, app], {
    encoding: 'utf8',
    env: { ...process.env, CARGO_TARGET_DIR: join(work, 'no-target'), SIDECARS: 'fake-sidecar', ...env },
  });
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
  const r = verify(bundle(), { PATH: `${vendorTeamShim()}:${process.env.PATH}` });
  assert.notEqual(r.status, 0, r.out);
  assert.match(r.out, /fake-sidecar has TeamIdentifier 'VENDOR1234', the app has 'none'/);
});

test('a sidecar that does not start from the built app fails', { skip }, () => {
  const r = verify(bundle({ sidecarExit: 3 }));
  assert.notEqual(r.status, 0, r.out);
  assert.match(r.out, /the sidecar fake-sidecar does not start/);
});

test('the sidecars default to bundle.externalBin from tauri.conf.json', { skip }, () => {
  const conf = JSON.parse(readFileSync(join(root, 'app', 'src-tauri', 'tauri.conf.json'), 'utf8'));
  const first = (conf.bundle.externalBin ?? [])[0]?.split('/').pop();
  const { SIDECARS: _, ...env } = process.env;
  const r = spawnSync('bash', [script, bundle()], { encoding: 'utf8', env: { ...env, CARGO_TARGET_DIR: join(work, 'no-target') } });
  if (first) {
    assert.notEqual(r.status, 0, r.stdout);
    assert.match(`${r.stdout}${r.stderr}`, new RegExp(`the sidecar ${first} is missing`));
  } else {
    assert.equal(r.status, 0, `${r.stdout}${r.stderr}`);
  }
});

test('a missing bundle fails with a message', { skip }, () => {
  const r = verify(join(work, 'nothing', 'None.app'));
  assert.notEqual(r.status, 0);
  assert.match(r.out, /no \.app bundle found/);
});

test('the committed macOS bundle is ad hoc with the hardened runtime off (CI turns it on only for a real identity)', () => {
  const mac = JSON.parse(readFileSync(join(root, 'app', 'src-tauri', 'tauri.macos.conf.json'), 'utf8'));
  assert.equal(mac.bundle.macOS.signingIdentity, '-');
  assert.equal(mac.bundle.macOS.hardenedRuntime, false);
});

test('a relative bundle path is resolved before the script changes folder', { skip }, () => {
  const app = bundle();
  const r = spawnSync('bash', [script, 'Fake.app'], {
    cwd: dirname(app),
    encoding: 'utf8',
    env: { ...process.env, CARGO_TARGET_DIR: join(work, 'no-target'), SIDECARS: 'fake-sidecar' },
  });
  assert.equal(r.status, 0, `${r.stdout}${r.stderr}`);
});

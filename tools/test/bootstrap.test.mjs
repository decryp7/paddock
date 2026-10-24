import assert from 'node:assert/strict';
import { symlinkSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { calls, run, scratch, shim, write } from './helpers.mjs';

/** A repo with app/.nvmrc and a PATH of shims for a machine that has everything. */
function machine({ node = true, cargo = true } = {}) {
  const root = write(scratch(), { 'app/.nvmrc': '24\n' });
  const bin = scratch('paddock-bin-');
  if (node) symlinkSync(process.execPath, join(bin, 'node'));
  if (cargo) {
    shim(bin, 'cargo');
    write(bin, { rustc: '#!/bin/bash\necho "rustc 1.91.0 (abc 2026-01-01)"\n' });
  }
  write(bin, { sw_vers: '#!/bin/bash\necho 14.2\n', 'xcode-select': '#!/bin/bash\necho /Library/Developer/CommandLineTools\n' });
  shim(bin, 'npm');
  shim(bin, 'npx');
  return { root, bin };
}
const go = (m, args, env = {}) => run('bootstrap.sh', ['--name', 'Demo', '--root', m.root, ...args], { bin: m.bin, cwd: m.root, env: { PADDOCK_TOOLS_OS: 'Darwin', ...env } });

test('--check passes on a machine that has everything', () => {
  const m = machine();
  const r = go(m, ['--check']);
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /Demo macos bootstrap/);
  assert.match(r.out, /ok {2}macOS 14\.2/);
  assert.deepEqual(calls(m.bin), [], '--check builds nothing');
});

test('a missing tool is named with what to do, and nothing is built', () => {
  const m = machine({ cargo: false });
  const r = go(m, []);
  assert.equal(r.status, 1);
  assert.match(r.out, /missing {2}Rust\n {6}install rustup/);
  assert.deepEqual(calls(m.bin), []);
  const noNode = machine({ node: false });
  assert.match(go(noNode, ['--check']).out, /Node 24\n {6}install Node 24/);
});

test('the Node major must match app/.nvmrc', () => {
  const m = machine();
  write(m.root, { 'app/.nvmrc': '20\n' });
  const r = go(m, ['--check']);
  assert.equal(r.status, 1);
  assert.match(r.out, /Node 20\n {6}found Node v\d+/);
});

test('macOS older than the minimum fails; the minimum is a parameter', () => {
  const m = machine();
  write(m.bin, { sw_vers: '#!/bin/bash\necho 12.6\n' });
  assert.equal(go(m, ['--check']).status, 1);
  assert.equal(go(m, ['--check', '--min-macos', '12']).status, 0);
});

test('a build runs npm ci in app/, builds the macOS bundles and prints the next steps', () => {
  const m = machine();
  write(m.root, { 'next.txt': 'Next steps for Demo:\n  1. open it\n' });
  const r = go(m, ['--next', join(m.root, 'next.txt')], { CARGO_TARGET_DIR: join(m.root, 't') });
  assert.equal(r.status, 0, r.out);
  assert.deepEqual(calls(m.bin), ['npm ci --no-audit --no-fund', 'npx tauri build --bundles app,dmg -- --profile dist']);
  assert.match(r.out, /Built:\n {2}app: not found\n {2}dmg: not found/);
  assert.match(r.out, /Next steps for Demo:\n {2}1\. open it/);
});

test('without a next file the installed-app check is suggested', () => {
  const m = machine();
  assert.match(go(m, []).out, /verify-installed\.sh --name Demo --id demo/);
});

test('Linux: architecture and development packages are checked', () => {
  const m = machine();
  shim(m.bin, 'pkg-config', '[ "$2" != "librsvg-2.0" ]');
  const r = go(m, ['--check'], { PADDOCK_TOOLS_OS: 'Linux', PADDOCK_TOOLS_ARCH: 'x86_64' });
  assert.equal(r.status, 1);
  assert.match(r.out, /ok {2}webkit2gtk-4\.1 \(development files\)/);
  assert.match(r.out, /missing {2}librsvg-2\.0\n {6}sudo apt install libwebkit2gtk-4\.1-dev/);
  assert.match(go(m, ['--check'], { PADDOCK_TOOLS_OS: 'Linux', PADDOCK_TOOLS_ARCH: 'riscv64' }).out, /builds for x86_64 and aarch64 only \(found riscv64\)/);
  assert.match(go(machine(), ['--check'], { PADDOCK_TOOLS_OS: 'Linux', PADDOCK_TOOLS_ARCH: 'aarch64' }).out, /missing {2}pkg-config/);
});

test('Linux build uses the deb and AppImage bundles', () => {
  const m = machine();
  shim(m.bin, 'pkg-config');
  const r = go(m, [], { PADDOCK_TOOLS_OS: 'Linux', PADDOCK_TOOLS_ARCH: 'x86_64' });
  assert.equal(r.status, 0, r.out);
  assert.ok(calls(m.bin).includes('npx tauri build --bundles deb,appimage -- --profile dist'));
});

test('usage errors: no name, unknown option, no app folder, unsupported OS', () => {
  const m = machine();
  assert.equal(run('bootstrap.sh', ['--root', m.root], { bin: m.bin }).status, 2);
  assert.equal(go(m, ['--wat']).status, 2);
  assert.equal(run('bootstrap.sh', ['--name', 'Demo', '--root', scratch()], { bin: m.bin }).status, 2);
  const win = go(m, [], { PADDOCK_TOOLS_OS: 'MINGW64' });
  assert.equal(win.status, 1);
  assert.match(win.out, /not supported by this script/);
});

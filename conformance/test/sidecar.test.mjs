import assert from 'node:assert/strict';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { buildSidecars, findSidecars, hostTriple, main, parseSidecarSpec, planSidecars, profileDir, selfTestSidecars } from '../src/sidecar/sidecar.mjs';

const dyno = ['dyno-agent', 'dyno-helper', 'dyno=dyno-cli'].map(parseSidecarSpec);

test('specs: name, name=package, name=package:bin; anything else is refused', () => {
  assert.deepEqual(parseSidecarSpec('a'), { name: 'a', package: 'a', bin: 'a' });
  assert.deepEqual(parseSidecarSpec('dyno=dyno-cli'), { name: 'dyno', package: 'dyno-cli', bin: 'dyno' });
  assert.deepEqual(parseSidecarSpec('x=pkg:other'), { name: 'x', package: 'pkg', bin: 'other' });
  for (const bad of ['', 'a b', '../x', 'a=', 'a=b:', 'a=b:c:d', 'a/b']) assert.throws(() => parseSidecarSpec(bad), /not a sidecar spec/, bad);
});

test('hostTriple reads rustc -vV', () => {
  assert.equal(hostTriple(() => 'rustc 1.99\nhost: aarch64-apple-darwin\nrelease: 1.99\n'), 'aarch64-apple-darwin');
  assert.equal(hostTriple(() => 'nothing'), undefined);
});

test('plan: the host builds without --target, a cross build keeps it, universal builds both halves', () => {
  const base = { sidecars: dyno, host: 'aarch64-apple-darwin', targetDir: '/t', profile: 'release' };
  const native = planSidecars({ ...base, target: 'aarch64-apple-darwin' });
  assert.deepEqual(native.builds, [['build', '-p', 'dyno-agent', '--bin', 'dyno-agent', '-p', 'dyno-helper', '--bin', 'dyno-helper', '-p', 'dyno-cli', '--bin', 'dyno', '--release']]);
  assert.deepEqual(native.binaries.map((b) => [b.file, b.built]), [
    ['dyno-agent-aarch64-apple-darwin', ['/t/release/dyno-agent']],
    ['dyno-helper-aarch64-apple-darwin', ['/t/release/dyno-helper']],
    ['dyno-aarch64-apple-darwin', ['/t/release/dyno']],
  ]);
  const win = planSidecars({ ...base, target: 'x86_64-pc-windows-msvc' });
  assert.deepEqual(win.builds[0].slice(-3), ['--target', 'x86_64-pc-windows-msvc', '--release']);
  assert.equal(win.ext, '.exe');
  assert.equal(win.binaries[0].file, 'dyno-agent-x86_64-pc-windows-msvc.exe');
  assert.equal(win.binaries[0].built[0], join('/t', 'x86_64-pc-windows-msvc', 'release', 'dyno-agent.exe'));
  const uni = planSidecars({ ...base, sidecars: [parseSidecarSpec('a')], target: 'universal-apple-darwin' });
  assert.equal(uni.builds.length, 2);
  assert.deepEqual(uni.binaries[0].built, [join('/t', 'aarch64-apple-darwin', 'release', 'a'), join('/t', 'x86_64-apple-darwin', 'release', 'a')]);
});

test('plan: profiles and a manifest path', () => {
  const base = { sidecars: [parseSidecarSpec('a')], host: 'h', target: 'h', targetDir: '/t' };
  assert.deepEqual(planSidecars({ ...base, profile: 'debug' }).builds[0], ['build', '-p', 'a', '--bin', 'a']);
  assert.deepEqual(planSidecars({ ...base, profile: 'dist' }).builds[0], ['build', '-p', 'a', '--bin', 'a', '--profile', 'dist']);
  assert.deepEqual(planSidecars({ ...base, profile: 'dist' }).binaries[0].built, [join('/t', 'dist', 'a')]);
  assert.deepEqual(planSidecars({ ...base, profile: 'release', manifestPath: 'src-tauri/Cargo.toml' }).builds[0].slice(0, 3), ['build', '--manifest-path', 'src-tauri/Cargo.toml']);
  assert.equal(profileDir('dev'), 'debug');
  assert.throws(() => planSidecars({ ...base, sidecars: [], profile: 'release' }), /no sidecars/);
  assert.throws(() => planSidecars({ ...base, target: '', profile: 'release' }), /cannot tell the target/);
});

function fakeOps(sizes = {}) {
  const calls = [];
  return {
    calls,
    ops: {
      run: (cmd, args, cwd) => calls.push(['run', cmd, args, cwd]),
      copy: (a, b) => calls.push(['copy', a, b]),
      mkdir: (p) => calls.push(['mkdir', p]),
      chmod: (p, m) => calls.push(['chmod', p, m]),
      size: (p) => sizes[p] ?? 10,
      log: () => undefined,
    },
  };
}

test('build: runs cargo, copies each binary under its triple name and marks it executable', () => {
  const { calls, ops } = fakeOps();
  const written = buildSidecars({ root: '/r', out: '/r/binaries', sidecars: [parseSidecarSpec('svc')], target: 'x86_64-unknown-linux-gnu', host: 'x86_64-unknown-linux-gnu', ops });
  assert.deepEqual(written, [join('/r/binaries', 'svc-x86_64-unknown-linux-gnu')]);
  assert.deepEqual(calls.map((c) => c[0]), ['run', 'mkdir', 'copy', 'chmod']);
  assert.deepEqual(calls[0], ['run', 'cargo', ['build', '-p', 'svc', '--bin', 'svc', '--release'], '/r']);
  assert.deepEqual(calls[2], ['copy', join('/r', 'target', 'release', 'svc'), join('/r/binaries', 'svc-x86_64-unknown-linux-gnu')]);
});

test('build: an empty built file is an error, a universal build is joined with lipo, Windows files are not chmodded', () => {
  assert.throws(
    () => buildSidecars({ root: '/r', out: '/o', sidecars: [parseSidecarSpec('svc')], target: 'h', host: 'h', targetDir: '/t', ops: fakeOps({ [join('/t', 'release', 'svc')]: 0 }).ops }),
    /is empty/,
  );
  const uni = fakeOps();
  buildSidecars({ root: '/r', out: '/o', sidecars: [parseSidecarSpec('svc')], target: 'universal-apple-darwin', host: 'aarch64-apple-darwin', targetDir: '/t', ops: uni.ops });
  assert.ok(uni.calls.some((c) => c[0] === 'run' && c[1] === 'lipo' && c[2][0] === '-create'));
  const win = fakeOps();
  buildSidecars({ root: '/r', out: '/o', sidecars: [parseSidecarSpec('svc')], target: 'x86_64-pc-windows-msvc', host: 'x86_64-pc-windows-msvc', targetDir: '/t', ops: win.ops });
  assert.ok(!win.calls.some((c) => c[0] === 'chmod'));
});

test('build with the real file operations: cargo is the only thing faked', () => {
  const root = mkdtempSync(join(tmpdir(), 'sidecar-'));
  const built = join(root, 'target', 'release');
  mkdirSync(built, { recursive: true });
  writeFileSync(join(built, 'svc'), '#!/bin/sh\necho hi\n');
  const out = join(root, 'src-tauri', 'binaries');
  const written = buildSidecars({ root, out, sidecars: [parseSidecarSpec('svc')], target: 'x86_64-unknown-linux-gnu', host: 'x86_64-unknown-linux-gnu', ops: { run: () => undefined, log: () => undefined } });
  assert.ok(existsSync(written[0]));
  assert.ok(statSync(written[0]).mode & 0o100, 'executable');
});

test('findSidecars matches the bare name and the triple-suffixed one, and names what is missing', () => {
  const list = () => ['app', 'svc-aarch64-apple-darwin', 'helper.exe', 'other-x86_64-pc-windows-msvc.exe', 'svcx-aarch64-apple-darwin'];
  assert.deepEqual(findSidecars('/b', ['svc', 'helper', 'other'], list), [join('/b', 'svc-aarch64-apple-darwin'), join('/b', 'helper.exe'), join('/b', 'other-x86_64-pc-windows-msvc.exe')]);
  assert.throws(() => findSidecars('/b', ['nope'], list), /no sidecar nope in \/b/);
});

test('self-test: every sidecar is started with --self-test; a failure, a hang and a signal are reported', () => {
  const list = () => ['a', 'b', 'c', 'd'];
  const answers = {
    a: { status: 0, stdout: 'a 1.0 self-test\nok executable\n' },
    b: { status: 1, stdout: 'FAIL library: missing' },
    c: { status: null, signal: 'SIGTERM', error: new Error('ETIMEDOUT') },
    d: { status: 0, stdout: '' },
  };
  const seen = [];
  const results = selfTestSidecars({ dir: '/bundle', names: ['a', 'b', 'c', 'd'], list, spawn: (file, args, t) => (seen.push([file, args, t]), answers[file.slice(-1)]) });
  assert.deepEqual(results.map((r) => [r.name, r.ok]), [['a', true], ['b', false], ['c', false], ['d', true]]);
  assert.deepEqual(seen[0], [join('/bundle', 'a'), ['--self-test'], 30_000]);
  assert.match(results[2].output, /ETIMEDOUT.*SIGTERM/);
});

test('cli: build and self-test, exit codes and usage errors', () => {
  const logs = [];
  const errs = [];
  const io = { log: (m) => logs.push(m), error: (m) => errs.push(m), env: {}, host: () => 'h' };
  const { calls, ops } = fakeOps();
  assert.equal(main(['build', '--root', '/r', '--out', '/o', '--sidecar', 'svc', '--dev'], { ...io, ops }), 0);
  assert.deepEqual(calls[0].slice(0, 3), ['run', 'cargo', ['build', '-p', 'svc', '--bin', 'svc']]);
  const withEnv = fakeOps();
  main(['build', '--root', '/r', '--sidecar', 'svc'], { ...io, ops: withEnv.ops, env: { TAURI_ENV_TARGET_TRIPLE: 'x86_64-pc-windows-msvc', PADDOCK_CARGO_PROFILE: 'dist', CARGO_TARGET_DIR: '/ct' } });
  assert.deepEqual(withEnv.calls[0][2], ['build', '-p', 'svc', '--bin', 'svc', '--target', 'x86_64-pc-windows-msvc', '--profile', 'dist']);
  assert.equal(main(['self-test', '--dir', '/b', '--sidecar', 'a'], { ...io, list: () => ['a'], spawn: () => ({ status: 0, stdout: 'ok' }) }), 0);
  assert.equal(main(['self-test', '--dir', '/b', '--sidecar', 'a'], { ...io, list: () => ['a'], spawn: () => ({ status: 3, stdout: '' }) }), 1);
  assert.match(errs.at(-1), /FAIL a/);
  for (const bad of [[], ['nope', '--sidecar', 'a'], ['build'], ['build', '--sidecar'], ['build', '--what', '--sidecar', 'a'], ['self-test', '--sidecar', 'a'], ['build', '--sidecar', 'a b']]) {
    assert.equal(main(bad, io), 2, JSON.stringify(bad));
  }
  assert.match(errs.at(-1), /^paddock-sidecar: /);
});

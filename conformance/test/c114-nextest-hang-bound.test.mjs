import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { test } from 'node:test';
import { boundProblem, durationSeconds, overrideProblems } from '../src/checks/C114-nextest-hang-bound.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const git = (root, ...args) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.invalid', ...args], { cwd: root, encoding: 'utf8' });
const tracked = (files) => {
  const root = makeApp(files);
  git(root, 'init', '-q');
  git(root, 'add', '.');
  return root;
};
const WS = '[workspace]\nmembers = []\n';
const BOUNDED = '[profile.default]\nslow-timeout = { period = "30s", terminate-after = 4 }\n';

test('C114 reads durations and judges the default profile', () => {
  assert.equal(durationSeconds('30s'), 30);
  assert.equal(durationSeconds('2m'), 120);
  assert.ok(Number.isNaN(durationSeconds('soon')));
  assert.equal(boundProblem(BOUNDED), null);
  assert.equal(boundProblem('[profile.default]\nslow-timeout = { period = "60s", terminate-after = 2 }\n'), null);
  assert.match(boundProblem('[profile.ci]\nretries = 0\n'), /no \[profile\.default\]/);
  assert.match(boundProblem('[profile.default]\ntest-threads = -1\n'), /no slow-timeout/);
  assert.match(boundProblem('[profile.default]\nslow-timeout = "30s"\n'), /no terminate-after/);
  assert.match(boundProblem('[profile.default]\nslow-timeout = { period = "30s", terminate-after = 10 }\n'), /300 s/);
  // A terminate-after in the ci profile or an override does not bound the default profile.
  assert.match(boundProblem('[profile.default]\ntest-threads = -1\n[profile.ci]\nslow-timeout = { period = "30s", terminate-after = 2 }\n'), /no slow-timeout/);
});

test('C114 fails a workspace with no nextest config, or an unbounded or over-long default, and tells the next step', async () => {
  const none = await findingsOf('C114', tracked({ 'a/Cargo.toml': WS, 'a/Cargo.lock': '' }));
  assert.deepEqual(none.map((f) => [f.file, f.severity]), [['a', 'error']]);
  assert.match(none[0].fix, /terminate-after = 4/);
  const loose = await findingsOf('C114', tracked({ 'a/Cargo.toml': WS, 'a/Cargo.lock': '', 'a/.config/nextest.toml': '[profile.default]\ntest-threads = -1\n' }));
  assert.deepEqual(loose.map((f) => f.file), ['a/.config/nextest.toml']);
  assert.match(loose[0].fix, /named `\[\[profile\.default\.overrides\]\]`/);
  const long = await findingsOf('C114', tracked({ 'a/Cargo.toml': WS, 'a/Cargo.lock': '', 'a/.config/nextest.toml': '[profile.default]\nslow-timeout = { period = "30s", terminate-after = 10 }\n' }));
  assert.equal(long.length, 1);
});

test('C114 passes bounded workspaces, a demo that names a shared config, and ignores fuzz, vendor and non-workspace folders', async () => {
  const root = tracked({
    'crates/Cargo.toml': WS, 'crates/Cargo.lock': '', 'crates/.config/nextest.toml': BOUNDED,
    'crates/.config/nextest-demos.toml': BOUNDED,
    'apps/demo/paddock.ci.json': '{"nextestConfig": "crates/.config/nextest-demos.toml"}',
    'apps/demo/src-tauri/Cargo.toml': WS, 'apps/demo/src-tauri/Cargo.lock': '',
    'apps/demo2/paddock.ci.json': '{"nextestConfig": "crates/.config/nextest-demos.toml"}', // two apps name one config
    'apps/demo2/src-tauri/Cargo.toml': WS, 'apps/demo2/src-tauri/Cargo.lock': '',
    'apps/x/fuzz/Cargo.toml': WS, 'apps/x/fuzz/Cargo.lock': '',
    'apps/x/vendor/t/Cargo.toml': WS, 'apps/x/vendor/t/Cargo.lock': '',
    'b/Cargo.toml': '[package]\nname = "b"\n', 'b/Cargo.lock': '',
  });
  assert.deepEqual(await findingsOf('C114', root), []);
});

test('C114 fails a demo whose shared config is unbounded', async () => {
  const root = tracked({
    'crates/.config/nextest-demos.toml': '[profile.default]\ntest-threads = -1\n',
    'apps/demo/paddock.ci.json': '{"nextestConfig": "crates/.config/nextest-demos.toml"}',
    'apps/demo/src-tauri/Cargo.toml': WS, 'apps/demo/src-tauri/Cargo.lock': '',
  });
  assert.deepEqual((await findingsOf('C114', root)).map((f) => f.file), ['crates/.config/nextest-demos.toml']);
});

test('C114 outside a git checkout is "not measured", an error with the next step (ER-33)', async () => {
  const f = await findingsOf('C114', makeApp({ 'a/Cargo.toml': WS, 'a/Cargo.lock': '', 'a/.config/nextest.toml': BOUNDED }));
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'error');
  assert.match(f[0].message, /not measured/);
  assert.match(f[0].fix, /git checkout/);
});

test('C114 counts an untracked Cargo.lock beside a workspace manifest', async () => {
  const root = tracked({ 'x.txt': 'x\n' });
  const { mkdirSync, writeFileSync } = await import('node:fs');
  mkdirSync(`${root}/new`);
  writeFileSync(`${root}/new/Cargo.toml`, WS);
  writeFileSync(`${root}/new/Cargo.lock`, '');
  assert.deepEqual((await findingsOf('C114', root)).map((f) => f.file), ['new']);
});

const WAIVED = (extra) => `${BOUNDED}\n# waiver: why\n[[profile.default.overrides]]\nfilter = "all()"\n${extra}\n`;

test('C114 judges every override that sets a slow-timeout: waiver comment, terminate-after, cap, bench exempt from the cap', () => {
  assert.deepEqual(overrideProblems(WAIVED('slow-timeout = { period = "60s", terminate-after = 8 }')), []);
  assert.deepEqual(overrideProblems(WAIVED('test-group = "g"')), [], 'an override without a slow-timeout needs nothing');
  const bare = `${BOUNDED}\n[[profile.ci.overrides]]\nfilter = "all()"\nslow-timeout = { period = "30s", terminate-after = 2 }\n`;
  assert.match(overrideProblems(bare)[0][1], /ci override .*waiver/);
  const apart = `${BOUNDED}\n# waiver: why\n\n[[profile.ci.overrides]]\nslow-timeout = { period = "30s", terminate-after = 2 }\n`;
  assert.match(overrideProblems(apart)[0][1], /waiver/, 'a blank line separates the comment from the header');
  assert.match(overrideProblems(WAIVED('slow-timeout = "30s"'))[0][1], /no terminate-after/);
  assert.match(overrideProblems(WAIVED('slow-timeout = { period = "60s", terminate-after = 30 }'))[0][1], /1800 s/);
  const bench = '# waiver: benchmarks\n[[profile.bench.overrides]]\nfilter = "all()"\nslow-timeout = { period = "60s", terminate-after = 30 }\n';
  assert.deepEqual(overrideProblems(bench), []);
  assert.match(overrideProblems(bench.replace('terminate-after = 30 ', ''))[0][1], /no terminate-after/);
});

test('C114 reports an override problem through the runner with its line and the next step', async () => {
  const cfg = `${BOUNDED}\n[[profile.ci.overrides]]\nfilter = "all()"\nslow-timeout = { period = "30s", terminate-after = 2 }\n`;
  const f = await findingsOf('C114', tracked({ 'a/Cargo.toml': WS, 'a/Cargo.lock': '', 'a/.config/nextest.toml': cfg }));
  assert.deepEqual(f.map((x) => [x.file, x.line]), [['a/.config/nextest.toml', 4]]);
  assert.match(f[0].fix, /# waiver:/);
});

test('C114 reports a config that is in the index but gone from the working tree instead of crashing', async () => {
  const root = tracked({ 'a/Cargo.toml': WS, 'a/Cargo.lock': '', 'a/.config/nextest.toml': BOUNDED });
  const { rmSync } = await import('node:fs');
  rmSync(`${root}/a/.config/nextest.toml`);
  const f = await findingsOf('C114', root);
  assert.deepEqual(f.map((x) => x.file), ['a/.config/nextest.toml']);
  assert.match(f[0].message, /file is gone/);
  assert.match(f[0].fix, /git checkout|git rm/);
});

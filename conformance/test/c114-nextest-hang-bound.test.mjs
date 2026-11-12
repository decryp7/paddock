import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { test } from 'node:test';
import { boundProblem, durationSeconds } from '../src/checks/C114-nextest-hang-bound.mjs';
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
  assert.deepEqual(await findingsOf('C114', makeApp({ 'a/Cargo.toml': WS })), [], 'not a git checkout: nothing to check');
});

test('C114 fails a demo whose shared config is unbounded', async () => {
  const root = tracked({
    'crates/.config/nextest-demos.toml': '[profile.default]\ntest-threads = -1\n',
    'apps/demo/paddock.ci.json': '{"nextestConfig": "crates/.config/nextest-demos.toml"}',
    'apps/demo/src-tauri/Cargo.toml': WS, 'apps/demo/src-tauri/Cargo.lock': '',
  });
  assert.deepEqual((await findingsOf('C114', root)).map((f) => f.file), ['crates/.config/nextest-demos.toml']);
});

import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import {
  SHIPPED_DIRS,
  assertProbeSees,
  cratesIn,
  elevationOffences,
  featureTree,
  forbiddenIn,
  shipped,
  shippedPackages,
  startedByAgent,
  testSupportEnabledIn,
  tree,
} from './check-shipped.mjs';

const root = new URL('..', import.meta.url).pathname;

const SHIPPED_NOW = ['dyno-app', 'dyno-agent', 'dyno-cli', 'dyno-bench', 'dyno-bench-gpu', 'dyno-helper'];

test('the shipped set is derived from the bundle and includes the helper', () => {
  assert.deepEqual(shipped(root).sort(), [...SHIPPED_NOW].sort());
});

test('the shipped set follows tauri.conf.json: a new bundled binary is shipped, an unknown one fails', () => {
  const metadata = { packages: [{ name: 'a-pkg', targets: [{ name: 'a-bin', kind: ['bin'] }, { name: 'a_lib', kind: ['lib'] }] }] };
  assert.deepEqual(shippedPackages(metadata, { bundle: { externalBin: ['binaries/a-bin'] } }), ['dyno-app', 'a-pkg']);
  assert.throws(() => shippedPackages(metadata, { bundle: { externalBin: ['binaries/ghost'] } }), /ghost/);
  assert.throws(() => shippedPackages(metadata, { bundle: { externalBin: ['binaries/a_lib'] } }), /a_lib/);
});

test('no shipped binary other than the helper depends on the helper', () => {
  for (const pkg of shipped(root)) assert.deepEqual(forbiddenIn(tree(root, pkg), pkg), [], `${pkg} must not depend on dyno-helper`);
});

test('no shipped package enables test-support on the crates that hold the test doubles', () => {
  assertProbeSees(root);
  for (const pkg of shipped(root)) assert.deepEqual(testSupportEnabledIn(featureTree(root, pkg, 'normal')), [], `${pkg} must not enable test-support`);
});

test('the feature probe sees test-support when the dev edges are in, and reports check broken when it cannot', () => {
  const withIt = [
    'dyno-agent v1.1.0 (/x/a)',
    'dyno-platform v1.1.0 (/x/p) test-support',
    'dyno-core v1.1.0 (/x/c) test-support,ts',
    'paddock-power v0.1.0 (/y/power) test-support',
    'serde v1.0.0 derive,std',
  ].join('\n');
  assert.deepEqual(testSupportEnabledIn(withIt), ['dyno-core', 'dyno-platform', 'paddock-power']);
  assert.doesNotThrow(() => assertProbeSees(root, withIt));
  // A tree without the feature (the probe changed, or the feature was renamed): never a silent pass.
  const without = 'dyno-agent v1.1.0 (/x/a)\ndyno-platform v1.1.0 (/x/p)\ndyno-core v1.1.0 (/x/c) ts\n';
  assert.deepEqual(testSupportEnabledIn(without), []);
  assert.throws(() => assertProbeSees(root, without), /check broken/);
  // The feature on an unrelated crate does not count, and a path with a space still parses.
  assert.deepEqual(testSupportEnabledIn('other v1.0.0 test-support\ndyno-platform v1.1.0 (/x y/p) test-support\n'), ['dyno-platform']);
});

test('every executable the agent starts is bundled', () => {
  const conf = JSON.parse(readFileSync(join(root, 'app/src-tauri/tauri.conf.json'), 'utf8'));
  const bundled = conf.bundle.externalBin.map((p) => p.split('/').pop());
  const started = startedByAgent(root);
  assert.ok(started.length > 0, 'the agent starts at least the benchmark runner');
  for (const exe of started) assert.ok(bundled.includes(exe), `${exe} is started by dyno-agent but not in bundle.externalBin`);
});

test('no source of a shipped crate asks for administrator rights', () => {
  assert.deepEqual(elevationOffences(root, SHIPPED_DIRS), []);
});

test('the check reads a dependency tree and finds the helper and elevation calls', () => {
  const out = 'dyno-app v0.1.0 (/x)\ndyno-core v0.1.0 (/x)\nserde v1.0.0\ndyno-helper v0.1.0 (/x/h)\n';
  assert.deepEqual([...cratesIn(out)].sort(), ['dyno-app', 'dyno-core', 'dyno-helper', 'serde']);
  assert.deepEqual(forbiddenIn(out, 'dyno-app'), ['dyno-helper']);
  assert.deepEqual(forbiddenIn(out, 'dyno-helper'), []);
  assert.deepEqual(forbiddenIn('dyno-app v0.1.0\n', 'dyno-app'), []);
  const dir = mkdtempSync(join(tmpdir(), 'dyno-shipped-'));
  mkdirSync(join(dir, 'crates/a/src'), { recursive: true });
  writeFileSync(join(dir, 'crates/a/src/lib.rs'), '// "pkexec" in a comment is fine\nfn f() { run("pkexec"); }\nconst T: &str = "Install polkit (pkexec) first.";\nfn g() { verb("runas"); }\n');
  assert.deepEqual(elevationOffences(dir, ['crates/a']), ['crates/a/src/lib.rs:2 "pkexec"', 'crates/a/src/lib.rs:4 "runas"']);
});

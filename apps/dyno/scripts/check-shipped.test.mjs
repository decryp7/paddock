import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { SHIPPED, SHIPPED_DIRS, cratesIn, elevationOffences, forbiddenIn, startedByAgent, tree } from './check-shipped.mjs';

const root = new URL('..', import.meta.url).pathname;

test('no shipped binary depends on the helper', () => {
  for (const pkg of SHIPPED) assert.deepEqual(forbiddenIn(tree(root, pkg)), [], `${pkg} must not depend on dyno-helper`);
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
  assert.deepEqual(forbiddenIn(out), ['dyno-helper']);
  assert.deepEqual(forbiddenIn('dyno-app v0.1.0\n'), []);
  const dir = mkdtempSync(join(tmpdir(), 'dyno-shipped-'));
  mkdirSync(join(dir, 'crates/a/src'), { recursive: true });
  writeFileSync(join(dir, 'crates/a/src/lib.rs'), '// "pkexec" in a comment is fine\nfn f() { run("pkexec"); }\nconst T: &str = "Install polkit (pkexec) first.";\nfn g() { verb("runas"); }\n');
  assert.deepEqual(elevationOffences(dir, ['crates/a']), ['crates/a/src/lib.rs:2 "pkexec"', 'crates/a/src/lib.rs:4 "runas"']);
});

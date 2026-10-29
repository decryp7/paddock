import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { SHIPPED, SHIPPED_DIRS, cratesIn, elevationOffences, forbiddenIn, tree } from './check-shipped.mjs';

const root = new URL('..', import.meta.url).pathname;

test('no shipped binary depends on the helper', () => {
  for (const pkg of SHIPPED) assert.deepEqual(forbiddenIn(tree(root, pkg)), [], `${pkg} must not depend on dyno-helper`);
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

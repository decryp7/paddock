import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { SPARSE_MESSAGE, isSparse } from './require-full-tree.mjs';

const script = fileURLToPath(new URL('./require-full-tree.mjs', import.meta.url));
const NEEDED = ['tools/paddock-dev/Cargo.toml', 'conformance/package.json', '.gitea/workflows/ci.yml'];
const tree = (files) => {
  const dir = mkdtempSync(join(tmpdir(), 'sparse-'));
  for (const f of files) {
    mkdirSync(dirname(join(dir, f)), { recursive: true });
    writeFileSync(join(dir, f), 'x');
  }
  return dir;
};

test('a tree missing a needed folder stops the tests with one line', () => {
  const dir = tree(['conformance/package.json']);
  assert.equal(isSparse(dir), true);
  const r = spawnSync('node', [script, dir], { encoding: 'utf8' });
  assert.equal(r.status, 1);
  assert.equal(r.stderr.trim().split('\n').length, 1);
  assert.ok(r.stderr.includes(SPARSE_MESSAGE));
});

test('a non-cone sparse tree that has the paths passes, as the CI host clones do', () => {
  const dir = tree(NEEDED);
  const git = (...a) => execFileSync('git', ['-C', dir, ...a], { stdio: 'ignore' });
  git('init', '-q');
  git('config', 'core.sparseCheckout', 'true');
  git('config', 'core.sparseCheckoutCone', 'false');
  assert.equal(isSparse(dir), false);
  assert.equal(spawnSync('node', [script, dir]).status, 0);
});

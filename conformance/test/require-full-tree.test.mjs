import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { SPARSE_MESSAGE, isSparse } from './require-full-tree.mjs';

const script = fileURLToPath(new URL('./require-full-tree.mjs', import.meta.url));

test('a sparse checkout stops the tests with one line; a full one passes', () => {
  const dir = mkdtempSync(join(tmpdir(), 'sparse-'));
  const git = (...a) => execFileSync('git', ['-C', dir, ...a], { stdio: 'ignore' });
  git('init', '-q');
  assert.equal(isSparse(dir), false);
  assert.equal(spawnSync('node', [script, dir]).status, 0);
  git('sparse-checkout', 'init', '--cone');
  assert.equal(isSparse(dir), true);
  const r = spawnSync('node', [script, dir], { encoding: 'utf8' });
  assert.equal(r.status, 1);
  assert.equal(r.stderr.trim().split('\n').length, 1);
  assert.ok(r.stderr.includes(SPARSE_MESSAGE));
});

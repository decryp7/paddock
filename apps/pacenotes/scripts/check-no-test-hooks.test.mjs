import assert from 'node:assert/strict';
import { test } from 'node:test';
import { check, verdict } from './check-no-test-hooks.mjs';

test('a tree without the feature passes', () => {
  assert.equal(verdict({ status: 0, stdout: 'pacenotes-worker v0.1.0\n└── pacenotes-index v0.1.0\n' }).ok, true);
});

test('the feature on in a build without dev-dependencies fails and shows the tree', () => {
  const r = verdict({ status: 0, stdout: 'pacenotes-worker v0.1.0\n├── pacenotes-worker feature "test-hooks"\n' });
  assert.equal(r.ok, false);
  assert.match(r.message, /test-hooks/);
});

test('a cargo failure fails: the guard never passes on an error', () => {
  const r = verdict({ status: 101, stderr: 'error: failed to parse manifest' });
  assert.equal(r.ok, false);
  assert.match(r.message, /cannot say/);
});

test('check runs cargo through the injected runner in the given folder', () => {
  let seen = null;
  const r = check('/x', (cwd) => { seen = cwd; return { status: 0, stdout: '' }; });
  assert.deepEqual([seen, r.ok], ['/x', true]);
});

test('the real tree of this workspace has the feature off', () => {
  assert.equal(check(new URL('..', import.meta.url).pathname).ok, true);
});

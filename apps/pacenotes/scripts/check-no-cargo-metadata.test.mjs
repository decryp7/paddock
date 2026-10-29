import assert from 'node:assert/strict';
import { test } from 'node:test';
import { check, verdict } from './check-no-cargo-metadata.mjs';

test('a package that is not in the graph passes', () => {
  const r = verdict({ status: 101, stderr: 'error: package ID specification `paddock-repo-graph` did not match any packages' });
  assert.equal(r.ok, true);
});

test('the package in the graph with the feature off passes', () => {
  assert.equal(verdict({ status: 0, stdout: 'paddock-repo-graph v0.1.0\n└── pacenotes-graph v0.1.0\n' }).ok, true);
});

test('the feature on, directly or through another crate, fails and shows the tree', () => {
  const tree = 'paddock-repo-graph v0.1.0\n├── paddock-repo-graph feature "cargo-metadata"\n│   └── pacenotes-graph feature "meta"\n';
  const r = verdict({ status: 0, stdout: tree });
  assert.equal(r.ok, false);
  assert.match(r.message, /cargo-metadata/);
});

test('any other cargo failure fails: the guard never passes on an error it does not understand', () => {
  const r = verdict({ status: 101, stderr: 'error: failed to parse manifest' });
  assert.equal(r.ok, false);
  assert.match(r.message, /cannot say/);
});

test('check runs cargo through the injected runner in the given folder', () => {
  let seen = null;
  const r = check('/x', (cwd) => { seen = cwd; return { status: 0, stdout: 'paddock-repo-graph v0.1.0\n' }; });
  assert.deepEqual([seen, r.ok], ['/x', true]);
});

test('cargo is asked about every target, so a cfg(...) dependency is seen', async () => {
  const src = await import('node:fs').then((fs) => fs.readFileSync(new URL('./check-no-cargo-metadata.mjs', import.meta.url), 'utf8'));
  assert.match(src, /'--target', 'all'/);
});

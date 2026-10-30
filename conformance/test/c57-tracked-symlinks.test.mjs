import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, symlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { trackedSymlinks } from '../src/checks/C57-tracked-symlinks.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const git = (root, ...args) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.invalid', ...args], { cwd: root, encoding: 'utf8' });

test('C57 lists only mode 120000 entries, minus the allow-list', () => {
  const out = '100644 aaaa 0\ta.txt\n120000 bbbb 0\tnode_modules\n100755 cccc 0\trun.sh\n120000 dddd 0\tdocs/x link\n160000 eeee 0\tvendor/sub\n';
  assert.deepEqual(trackedSymlinks(out), ['node_modules', 'docs/x link']);
  assert.deepEqual(trackedSymlinks(out, ['node_modules']), ['docs/x link']);
  assert.deepEqual(trackedSymlinks(''), []);
});

test('C57 through the runner: a committed symlink is an error, plain files and untracked links are not', async () => {
  const root = makeApp({ 'a.txt': 'x\n' });
  git(root, 'init', '-q');
  symlinkSync('/elsewhere', join(root, 'node_modules'));
  writeFileSync(join(root, 'b.txt'), 'y\n');
  git(root, 'add', 'a.txt', 'node_modules');
  const f = await findingsOf('C57', root);
  assert.deepEqual(f.map((x) => [x.file, x.severity]), [['node_modules', 'error']]);
  assert.match(f[0].fix, /node_modules\/?`/);
  const clean = makeApp({ 'a.txt': 'x\n' });
  git(clean, 'init', '-q');
  git(clean, 'add', 'a.txt');
  assert.deepEqual(await findingsOf('C57', clean), []);
  assert.deepEqual(await findingsOf('C57', makeApp({ 'a.txt': 'x\n' })), [], 'not a git checkout: nothing to check');
});

test('C57 checks a folder inside a git repository: only its own tracked symlinks', async () => {
  const repo = makeApp({ 'apps/a/x.txt': 'x\n', 'apps/b/y.txt': 'y\n' });
  git(repo, 'init', '-q');
  mkdirSync(join(repo, 'apps/a/app'), { recursive: true });
  symlinkSync('/elsewhere', join(repo, 'apps/a/app/node_modules'));
  symlinkSync('/elsewhere', join(repo, 'apps/b/link'));
  git(repo, 'add', '.');
  const f = await findingsOf('C57', join(repo, 'apps/a'));
  assert.deepEqual(f.map((x) => x.file), ['app/node_modules']);
});

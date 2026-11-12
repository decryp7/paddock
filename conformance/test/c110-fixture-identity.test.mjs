import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { test } from 'node:test';
import { identityHits, isPlaceholder } from '../src/checks/C110-fixture-identity.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const git = (root, ...args) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.invalid', ...args], { cwd: root, encoding: 'utf8' });

test('C110 placeholders are an allow-list; a real-looking value is not one', () => {
  for (const ok of ['', 'user', 'example', 'Example User', 'runner-1', 'runner-12', 'a@example.com', 'https://gitea.invalid/avatar.png', 'example/paddock', 'undefined']) {
    assert.equal(isPlaceholder(ok), true, ok);
  }
  for (const bad of ['jdoe', 'Jane Doe', 'jane@corp.net', 'https://host.net/avatars/ab12', 'box-9', 'my-laptop.local']) {
    assert.equal(isPlaceholder(bad), false, bad);
  }
});

test('C110 finds identity keys at any depth, in objects and arrays, and ignores other keys', () => {
  const j = { actor: { full_name: 'Jane Doe', login: 'user' }, jobs: [{ runner_name: 'box-9', name: 'Jane Doe' }], email: 'x@corp.net', avatar_url: 'https://gitea.invalid/a.png' };
  assert.deepEqual(identityHits(j).map((h) => h.path), ['.actor.full_name', '.jobs[0].runner_name', '.email']);
  assert.deepEqual(identityHits({ n: 1, login: null, hostname: 3 }), []);
});

test('C110 through the runner: a fixture with a real identity is an error, a placeholder one and a non-fixture file are not', async () => {
  const root = makeApp({
    'tools/x/tests/bad/run.json': JSON.stringify({ actor: { full_name: 'Jane Doe' } }),
    'tools/x/tests/good/run.json': JSON.stringify({ actor: { full_name: 'Example User', email: 'u@example.com' } }),
    'tools/x/src/data.json': JSON.stringify({ full_name: 'Jane Doe' }),
    'apps/a/fixtures/h.json': JSON.stringify({ hostname: 'mac-mini' }),
    'tools/x/tests/broken.json': '{ not json',
  });
  git(root, 'init', '-q');
  git(root, 'add', '.');
  const f = await findingsOf('C110', root);
  assert.deepEqual(f.map((x) => [x.file, x.severity]).sort(), [['apps/a/fixtures/h.json', 'error'], ['tools/x/tests/bad/run.json', 'error']]);
  assert.match(f[0].message, /full_name|hostname/);
  assert.ok(!f.some((x) => /Jane/.test(x.message)), 'the finding never repeats the value');
});

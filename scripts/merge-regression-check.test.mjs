import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const script = fileURLToPath(new URL('./merge-regression-check.mjs', import.meta.url));

// Invented history in a temporary repository; nothing here is real data.
function repo() {
  const dir = mkdtempSync(join(tmpdir(), 'mrc-'));
  const git = (...a) => execFileSync('git', ['-C', dir, '-c', 'user.name=t', '-c', 'user.email=t@example.invalid', '-c', 'commit.gpgsign=false', ...a], { encoding: 'utf8' }).trim();
  git('init', '-q', '-b', 'main');
  const write = (f, text) => { mkdirSync(join(dir, f, '..'), { recursive: true }); writeFileSync(join(dir, f), text); };
  const commit = (msg) => { git('add', '-A'); git('commit', '-q', '-m', msg); return git('rev-parse', 'HEAD'); };
  return { dir, git, write, commit, done: () => rmSync(dir, { recursive: true, force: true }) };
}
const lines = (n, p) => Array.from({ length: n }, (_, i) => `${p} line ${i + 1} of the original file`).join('\n') + '\n';
const run = (dir, ...args) => spawnSync('node', [script, '--repo', dir, ...args], { encoding: 'utf8' });

test('a branch commit built on a stale tree that silently reverts a fix is flagged', () => {
  const r = repo();
  try {
    r.write('src/a.txt', lines(40, 'a'));
    r.write('src/b.txt', lines(10, 'b'));
    const stale = r.commit('initial');
    r.write('src/a.txt', lines(40, 'a') + 'fix: release the buffer\nfix: redact the token\nfix: bound the cache\n');
    r.commit('fix: memory and redaction');
    const fixed = r.git('rev-parse', 'HEAD');
    // The branch commit has the fixed commit as its parent but a tree made from the stale one plus its own change.
    r.git('checkout', '-q', '-b', 'feature', fixed);
    r.git('checkout', stale, '--', 'src/a.txt');
    r.write('src/new.txt', 'the feature\n');
    r.commit('feat: the feature');
    r.git('checkout', '-q', 'main');
    r.write('src/b.txt', lines(10, 'b') + 'unrelated main work\n');
    r.commit('chore: main moves on');
    r.git('merge', '-q', '--no-ff', '-m', 'Merge feature', 'feature');
    const res = run(r.dir, 'HEAD');
    assert.equal(res.status, 1, res.stdout + res.stderr);
    assert.match(res.stdout, /src\/a\.txt/);
    assert.doesNotMatch(res.stdout, /src\/new\.txt|src\/b\.txt/);
    const errors = res.stdout.split('\n').filter((l) => l.startsWith('::error::'));
    assert.equal(errors.length, 1);
    assert.match(errors[0], /src\/a\.txt/);
    assert.match(errors[0], /next step/i);
    // The branch and target form flags it too, without a merge commit.
    const res2 = run(r.dir, '--branch', 'feature', '--target', 'HEAD^');
    assert.equal(res2.status, 1, res2.stdout + res2.stderr);
  } finally { r.done(); }
});

test('a merge that changes a file the branch never touched is flagged', () => {
  const r = repo();
  try {
    r.write('a.txt', lines(20, 'a'));
    r.write('b.txt', lines(20, 'b'));
    r.commit('initial');
    r.git('checkout', '-q', '-b', 'feature');
    r.write('b.txt', lines(20, 'b') + 'branch line\n');
    r.commit('feat: b');
    r.git('checkout', '-q', 'main');
    r.write('a.txt', lines(20, 'a') + 'main line\n');
    r.commit('fix: a');
    r.git('merge', '-q', '--no-ff', '--no-commit', 'feature');
    r.write('a.txt', lines(20, 'a')); // an evil merge drops the main line
    r.git('add', '-A'); r.git('commit', '-q', '-m', 'Merge feature');
    const res = run(r.dir, 'HEAD');
    assert.equal(res.status, 1, res.stdout + res.stderr);
    assert.match(res.stdout, /a\.txt/);
  } finally { r.done(); }
});

test('a normal merge is not flagged', () => {
  const r = repo();
  try {
    r.write('src/a.txt', lines(40, 'a'));
    r.write('src/b.txt', lines(10, 'b'));
    r.commit('initial');
    r.git('checkout', '-q', '-b', 'feature');
    // The branch edits two old lines of a.txt, adds a line and a file.
    r.write('src/a.txt', lines(40, 'a').replace('a line 3 of the original file', 'a line 3 changed by the branch').replace('a line 4 of the original file', 'a line 4 changed by the branch') + 'branch tail\n');
    r.write('src/new.txt', 'the feature\n');
    r.commit('feat: feature');
    r.git('checkout', '-q', 'main');
    r.write('src/b.txt', lines(10, 'b') + 'main work\n');
    r.commit('fix: main work');
    r.write('src/a.txt', 'main head\n' + lines(40, 'a'));
    r.commit('fix: head of a');
    r.git('merge', '-q', '--no-ff', '-m', 'Merge feature', 'feature');
    const res = run(r.dir, 'HEAD');
    assert.equal(res.status, 0, res.stdout + res.stderr);
    assert.doesNotMatch(res.stdout, /::error::/);
  } finally { r.done(); }
});

test('a commit that is not a merge is a usage error', () => {
  const r = repo();
  try {
    r.write('a.txt', 'x\n'); r.commit('initial');
    const res = run(r.dir, 'HEAD');
    assert.equal(res.status, 2);
    assert.match(res.stdout + res.stderr, /::error::/);
  } finally { r.done(); }
});

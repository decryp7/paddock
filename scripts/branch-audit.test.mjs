// Tests of scripts/branch-audit.mjs on invented repos in a temp folder.
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { after, before, describe, test } from 'node:test';

const script = join(dirname(fileURLToPath(import.meta.url)), 'branch-audit.mjs');
const scratch = mkdtempSync(join(tmpdir(), 'paddock-branch-audit-'));
after(() => rmSync(scratch, { recursive: true, force: true }));

const git = (repo, ...a) => {
  const r = spawnSync('git', ['-C', repo, '-c', 'user.name=t', '-c', 'user.email=t@example.com', ...a], { encoding: 'utf8' });
  assert.equal(r.status, 0, `git ${a.join(' ')}: ${r.stderr}`);
  return r.stdout.trim();
};
const write = (repo, file, text) => {
  mkdirSync(dirname(join(repo, file)), { recursive: true });
  writeFileSync(join(repo, file), text);
};
/** Commits `file` with `text` on the current branch. */
const commit = (repo, file, text, msg) => {
  write(repo, file, text);
  git(repo, 'add', '-A');
  git(repo, 'commit', '-q', '-m', msg);
};
/** Publishes the current branch as origin/<name> (a plain ref: no server needed). */
const publish = (repo, name) => git(repo, 'update-ref', `refs/remotes/origin/${name}`, 'HEAD');
const audit = (repo, ...args) => spawnSync('node', [script, '--repo', repo, '--app', 'x', '--ref', 'main', ...args], { encoding: 'utf8' });

const base = 'one\ntwo\nthree\nfour\nfive\n';
let repo;
before(() => {
  repo = join(scratch, 'repo');
  mkdirSync(repo);
  git(repo, 'init', '-q', '-b', 'main');
  commit(repo, 'apps/x/a.txt', base, 'init');
  commit(repo, 'apps/y/other.txt', 'other\n', 'other app');
  // merged: branch commit reachable from main
  git(repo, 'checkout', '-q', '-b', 'merged');
  commit(repo, 'apps/x/m.txt', 'merged line one\nmerged line two\n', 'merged work');
  publish(repo, 'merged');
  git(repo, 'checkout', '-q', 'main');
  git(repo, 'merge', '-q', '--no-ff', '-m', 'merge merged', 'merged');
  // cherry: same patch under another hash
  git(repo, 'checkout', '-q', '-b', 'cherry', 'main~1');
  commit(repo, 'apps/x/c.txt', 'cherry line one\ncherry line two\n', 'cherry work');
  publish(repo, 'cherry');
  git(repo, 'checkout', '-q', 'main');
  git(repo, 'cherry-pick', 'cherry');
  // content: main got the same lines by hand, in a different commit with different context
  git(repo, 'checkout', '-q', '-b', 'bycontent', 'main~2');
  commit(repo, 'apps/x/d.txt', 'content line one\ncontent line two\n', 'content work');
  publish(repo, 'bycontent');
  git(repo, 'checkout', '-q', 'main');
  commit(repo, 'apps/x/d.txt', 'header\ncontent line one\ncontent line two\nfooter\n', 'same lines, by hand');
  // second round: first commit merged, second never
  git(repo, 'checkout', '-q', '-b', 'second', 'main');
  commit(repo, 'apps/x/s.txt', 'round one line\nround one more\n', 'round one');
  git(repo, 'checkout', '-q', 'main');
  git(repo, 'merge', '-q', '--no-ff', '-m', 'merge round one', 'second');
  git(repo, 'checkout', '-q', 'second');
  commit(repo, 'apps/x/s.txt', 'round one line\nround one more\nround two line\n', 'round two');
  publish(repo, 'second');
  // allow-listed: never merged, but excused
  git(repo, 'checkout', '-q', '-b', 'dropped', 'main');
  commit(repo, 'apps/x/z.txt', 'dropped line one\ndropped line two\n', 'dropped work');
  publish(repo, 'dropped');
  // other app only: must not show
  git(repo, 'checkout', '-q', '-b', 'otherapp', 'main');
  commit(repo, 'apps/y/o.txt', 'other app line\nanother line\n', 'other app work');
  publish(repo, 'otherapp');
  git(repo, 'checkout', '-q', 'main');
  write(repo, 'apps/x/docs/branch-audit-allow.txt', '# branch  reason\ndropped  deliberately dropped, see the plan\n');
});

describe('branch audit', () => {
  test('flags the second-round commit and nothing already applied', () => {
    const r = audit(repo, '--allow', join(repo, 'apps/x/docs/branch-audit-allow.txt'));
    assert.equal(r.status, 1, r.stdout + r.stderr);
    const rows = Object.fromEntries(r.stdout.split('\n').filter((l) => l.startsWith('| ')).map((l) => l.split('|').map((c) => c.trim())).map((c) => [c[1].replace('origin/', ''), c]));
    assert.ok(rows['second'], r.stdout);
    assert.equal(rows['second'][2], 'missing');
    assert.equal(rows['cherry'][2], 'applied');
    assert.equal(rows['bycontent'][2], 'applied');
    assert.equal(rows['dropped'][2], 'allowed');
    assert.match(rows['dropped'][5], /deliberately dropped/);
    assert.equal(rows['merged'], undefined);
    assert.equal(rows['otherapp'], undefined);
    const errors = r.stdout.split('\n').filter((l) => l.startsWith('::error::'));
    assert.equal(errors.length, 1);
    assert.match(errors[0], /origin\/second/);
    assert.match(errors[0], /round two/);
  });

  test('passes with the missing branch allow-listed too', () => {
    const f = join(scratch, 'allow2.txt');
    writeFileSync(f, 'dropped  postponed\nsecond  round two postponed to 1.1\n');
    const r = audit(repo, '--allow', f);
    assert.equal(r.status, 0, r.stdout + r.stderr);
  });

  test('an allow-list entry without a reason is a usage error', () => {
    const f = join(scratch, 'allow3.txt');
    writeFileSync(f, 'dropped\n');
    assert.equal(audit(repo, '--allow', f).status, 2);
  });

  test('--since skips branches not updated since then', () => {
    const r = audit(repo, '--since', '2999-01-01T00:00:00Z');
    assert.equal(r.status, 0, r.stdout + r.stderr);
  });
});

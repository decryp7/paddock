import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { after, test } from 'node:test';
import { SCRATCH_ROOT, git, gitResult, scratchEnv } from './lib/scratch-git.mjs';

const LIB = resolve(import.meta.dirname, 'lib/scratch-git.mjs');
const scratch = mkdtempSync(join(tmpdir(), 'scratch-git-test-'));
after(() => rmSync(scratch, { recursive: true, force: true }));
// conformance: raw-git — the negative control: a raw git must be seen leaking under a hostile GIT_DIR, or the helper's test proves nothing (ER-33)
const raw = (cwd, args, env) => spawnSync('git', args, { cwd, env: { ...process.env, GIT_CONFIG_GLOBAL: '/dev/null', ...env }, encoding: 'utf8' });
/** A stand-in for the real repository: its config text, refs and HEAD, to compare before and after. */
const snapshot = (repo) => ({
  config: readFileSync(join(repo, '.git/config'), 'utf8'),
  head: readFileSync(join(repo, '.git/HEAD'), 'utf8'),
  refs: readdirSync(join(repo, '.git/refs/heads')).sort(),
});
function fakeReal() {
  const repo = mkdtempSync(join(scratch, 'fake-real-'));
  assert.equal(raw(repo, ['init', '-q', '-b', 'main']).status, 0);
  return repo;
}

test('positive control: a raw git under a hostile GIT_DIR and GIT_WORK_TREE writes into the repository they name', () => {
  const real = fakeReal();
  const elsewhere = mkdtempSync(join(scratch, 'raw-'));
  const before = snapshot(real);
  const hostile = { GIT_DIR: join(real, '.git'), GIT_WORK_TREE: real };
  assert.equal(raw(elsewhere, ['config', 'user.name', 'leak'], hostile).status, 0);
  assert.notEqual(snapshot(real).config, before.config, 'the probe can see a leak: without the helper the config of the named repository changes');
});

test('the helper under the same hostile environment leaves that repository untouched and works in its own scratch folder', () => {
  const real = fakeReal();
  const before = snapshot(real);
  const work = mkdtempSync(join(scratch, 'work-'));
  const script = `
    import { writeFileSync } from 'node:fs';
    import { join } from 'node:path';
    import { git } from ${JSON.stringify(LIB)};
    const w = ${JSON.stringify(work)};
    git(w, 'init', '-q');
    git(w, 'config', 'core.sparseCheckout', 'true');
    writeFileSync(join(w, 'a.txt'), 'x');
    git(w, 'add', '-A');
    git(w, 'commit', '-q', '-m', 'one');
    git(w, 'checkout', '-q', '-b', 'topic');
    console.log(git(w, 'rev-parse', '--absolute-git-dir').trim());
  `;
  const r = spawnSync(process.execPath, ['--input-type=module', '-e', script], {
    encoding: 'utf8',
    env: { ...process.env, GIT_DIR: join(real, '.git'), GIT_WORK_TREE: real, GIT_INDEX_FILE: join(real, '.git/index') },
  });
  assert.equal(r.status, 0, r.stderr);
  assert.ok(r.stdout.trim().startsWith(SCRATCH_ROOT), 'the repository it made is inside the scratch folder');
  assert.deepEqual(snapshot(real), before, 'the named repository: config, HEAD and refs unchanged');
  assert.doesNotMatch(snapshot(real).config, /worktree|user/);
});

test('scratchEnv drops GIT_* and pins the config; a redirecting variable in `env` is refused', () => {
  process.env.GIT_DIR = '/nonexistent';
  try {
    const env = scratchEnv({ HOME: '/x' });
    assert.equal(env.GIT_DIR, undefined);
    assert.equal(env.GIT_CONFIG_GLOBAL, '/dev/null');
    assert.equal(env.HOME, '/x');
  } finally {
    delete process.env.GIT_DIR;
  }
  assert.throws(() => scratchEnv({ GIT_DIR: '/x' }), /would point git at another repository/);
  assert.throws(() => scratchEnv({ GIT_CONFIG_KEY_0: 'x' }), /refused/);
});

test('refuses to run outside the temp folder and in a folder whose repository is not inside it', () => {
  assert.throws(() => git(resolve(import.meta.dirname, '..', '..'), 'status'), /not inside the temp folder/);
  const bare = mkdtempSync(join(scratch, 'norepo-'));
  assert.throws(() => git(bare, 'status'), /not one inside|rev-parse|run `git init`/);
  assert.equal(gitResult(bare, ['init', '-q']).status, 0);
  assert.match(git(bare, 'rev-parse', '--absolute-git-dir'), new RegExp(SCRATCH_ROOT.replace(/[/\\]/g, '.')));
});

test('a git failure throws with the command and stderr; gitResult reports it instead', () => {
  const d = mkdtempSync(join(scratch, 'fail-'));
  git(d, 'init', '-q');
  assert.throws(() => git(d, 'checkout', 'no-such-branch'), /exited 1|exited 128/);
  assert.notEqual(gitResult(d, ['checkout', 'no-such-branch']).status, 0);
  writeFileSync(join(d, 'f'), 'x');
  git(d, 'add', '-A');
  git(d, 'commit', '-q', '-m', 'c');
  assert.equal(git(d, 'log', '--format=%an').trim(), 't', 'the identity comes from -c');
});

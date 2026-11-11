import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { main } from '../src/cli.mjs';
import { fullCheckoutProblem } from '../src/util/full-checkout.mjs';

const git = (cwd, ...a) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.test', '-c', 'commit.gpgsign=false', ...a], { cwd, stdio: 'pipe', encoding: 'utf8' });

/** A throw-away repository with folders `apps/one`, `apps/two`, `crates` and `ui`, optionally sparse on `crates`. */
function repo({ sparse }) {
  const dir = mkdtempSync(join(tmpdir(), 'full-checkout-'));
  git(dir, 'init', '-q', '-b', 'main');
  for (const d of ['apps/one', 'apps/two', 'crates', 'ui']) {
    mkdirSync(join(dir, d), { recursive: true });
    writeFileSync(join(dir, d, 'f.txt'), 'x');
  }
  git(dir, 'add', '.');
  git(dir, 'commit', '-q', '-m', 'init');
  if (sparse) git(dir, 'sparse-checkout', 'set', '--cone', 'crates', 'apps/one');
  return dir;
}

test('a full checkout has no problem', () => {
  assert.equal(fullCheckoutProblem(repo({ sparse: false }), {}), null);
});

test('a sparse checkout names every missing folder and the command that fixes it', () => {
  const p = fullCheckoutProblem(repo({ sparse: true }), {});
  assert.match(p, /^::error::/);
  assert.match(p, /apps\/two/);
  assert.match(p, /\bui\b/);
  assert.doesNotMatch(p, /apps\/one/);
  assert.match(p, /git sparse-checkout disable/);
});

test('a folder outside any repository is no problem', () => {
  assert.equal(fullCheckoutProblem(mkdtempSync(join(tmpdir(), 'no-repo-')), {}), null);
});

test('the conformance run stops with that error in a sparse checkout, before it reads anything', async () => {
  const dir = repo({ sparse: true });
  const errs = [];
  const status = await main(['--app', 'paddock', '--root', dir], { log: () => {}, err: (m) => errs.push(m), env: {} });
  assert.equal(status, 2);
  assert.match(errs.join('\n'), /::error::.*git sparse-checkout disable/s);
});

test('list and commands (read by `dev gates check`) stop the same way', async () => {
  const dir = repo({ sparse: true });
  const was = process.cwd();
  process.chdir(dir);
  try {
    for (const cmd of ['list', 'commands']) {
      const errs = [];
      assert.equal(await main([cmd, '--json'], { log: () => {}, err: (m) => errs.push(m), env: {} }), 2);
      assert.match(errs.join('\n'), /::error::.*git sparse-checkout disable/s);
    }
  } finally {
    process.chdir(was);
  }
});

test('a workflow run and PADDOCK_ALLOW_SPARSE are never judged (CI checks out sparse on purpose)', () => {
  const dir = repo({ sparse: true });
  assert.equal(fullCheckoutProblem(dir, { GITHUB_ACTIONS: 'true' }), null);
  assert.equal(fullCheckoutProblem(dir, { PADDOCK_ALLOW_SPARSE: '1' }), null);
  assert.match(fullCheckoutProblem(dir, {}), /::error::/);
});

test('run as a script it prints the line and exits 1 in a sparse checkout, 0 in a full one', () => {
  const script = new URL('../src/util/full-checkout.mjs', import.meta.url).pathname;
  const env = { ...process.env, GITHUB_ACTIONS: '', PADDOCK_ALLOW_SPARSE: '' };
  const run = (dir) => {
    try {
      return { code: 0, err: execFileSync('node', [script, dir], { env, stdio: 'pipe', encoding: 'utf8' }) };
    } catch (e) {
      return { code: e.status, err: String(e.stderr) };
    }
  };
  assert.equal(run(repo({ sparse: false })).code, 0);
  const r = run(repo({ sparse: true }));
  assert.equal(r.code, 1);
  assert.match(r.err, /::error::.*git sparse-checkout disable/s);
});

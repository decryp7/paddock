import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

const git = (cwd, ...a) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.test', '-c', 'commit.gpgsign=false', ...a], { cwd, stdio: 'pipe', encoding: 'utf8' });

test('affected.mjs in a sparse checkout prints the ::error:: naming the folders to add, not an ENOENT', () => {
  const dir = mkdtempSync(join(tmpdir(), 'affected-sparse-'));
  git(dir, 'init', '-q', '-b', 'main');
  for (const d of ['crates', 'ui', 'scripts', 'conformance/src/util']) {
    mkdirSync(join(dir, d), { recursive: true });
    writeFileSync(join(dir, d, 'f.txt'), 'x');
  }
  // The script as it is in this repository, in a copy that has its own root (affected.mjs's ROOT is its parent folder).
  copyFileSync(new URL('./affected.mjs', import.meta.url), join(dir, 'scripts/affected.mjs'));
  for (const f of ['is-main.mjs', 'full-checkout.mjs']) copyFileSync(new URL(`../conformance/src/util/${f}`, import.meta.url), join(dir, 'conformance/src/util', f));
  git(dir, 'add', '.');
  git(dir, 'commit', '-q', '-m', 'init');
  git(dir, 'sparse-checkout', 'set', '--cone', 'crates', 'scripts', 'conformance');
  const script = join(dir, 'scripts/affected.mjs');
  let out;
  try {
    execFileSync('node', [script, '--all'], { cwd: dir, stdio: 'pipe', encoding: 'utf8', env: { ...process.env, GITHUB_ACTIONS: '', PADDOCK_ALLOW_SPARSE: '' } });
    assert.fail('expected a failure');
  } catch (e) {
    out = `${e.stderr}${e.stdout}`;
  }
  assert.match(out, /::error::.*\bui\b.*git sparse-checkout disable/s);
});

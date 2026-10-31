import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const SCRIPT = fileURLToPath(new URL('./bisect-flaky.sh', import.meta.url));
const run = (args, env = {}) => spawnSync('bash', [SCRIPT, ...args], { encoding: 'utf8', env: { ...process.env, ...env } });

test('exit codes: pass is 0, one failure is 1, a failed build is 125, bad usage is 2', () => {
  assert.equal(run(['5', 'true']).status, 0);
  assert.equal(run(['5', 'false']).status, 1);
  assert.equal(run(['3', 'true'], { BISECT_BUILD_CMD: 'false' }).status, 125);
  assert.equal(run(['3', 'true'], { BISECT_BUILD_CMD: 'true' }).status, 0);
  assert.equal(run(['x', 'true']).status, 2);
  assert.equal(run(['0', 'true']).status, 2);
  assert.equal(run(['3']).status, 2);
  assert.match(run(['3']).stderr, /::error::usage/);
});

test('stops at the first failure and runs exactly n times when green', () => {
  const dir = mkdtempSync(join(tmpdir(), 'bisect-count-'));
  try {
    const count = join(dir, 'count');
    const cmd = join(dir, 'cmd.sh');
    writeFileSync(cmd, `#!/bin/sh\necho x >> "${count}"\n[ "$(wc -l < "${count}")" -lt "$FAIL_AT" ]\n`);
    chmodSync(cmd, 0o755);
    assert.equal(run(['4', cmd], { FAIL_AT: '99' }).status, 0);
    assert.equal(execFileSync('wc', ['-l', count], { encoding: 'utf8' }).trim().split(/\s+/)[0], '4');
    rmSync(count);
    assert.equal(run(['10', cmd], { FAIL_AT: '3' }).status, 1);
    assert.equal(execFileSync('wc', ['-l', count], { encoding: 'utf8' }).trim().split(/\s+/)[0], '3');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('git bisect run finds the commit that introduced a flake (fails every third run)', () => {
  const dir = mkdtempSync(join(tmpdir(), 'bisect-git-'));
  const git = (...a) => execFileSync('git', ['-C', dir, '-c', 'user.name=t', '-c', 'user.email=t@example.invalid', '-c', 'commit.gpgsign=false', ...a], { encoding: 'utf8' });
  try {
    const counter = join(dir, '..', `${dir.split('/').pop()}.counter`);
    const testScript = (flaky) =>
      `#!/bin/sh\necho x >> "${counter}"\n${flaky ? `[ $(( $(wc -l < "${counter}") % 3 )) -ne 0 ]` : 'true'}\n`;
    git('init', '-q', '-b', 'main');
    const commit = (n, flaky) => {
      writeFileSync(join(dir, 't.sh'), testScript(flaky));
      chmodSync(join(dir, 't.sh'), 0o755);
      writeFileSync(join(dir, 'n.txt'), `${n}\n`);
      git('add', '-A');
      git('commit', '-q', '-m', `c${n}`);
      return git('rev-parse', 'HEAD').trim();
    };
    const good = commit(1, false);
    commit(2, false);
    const culprit = commit(3, true);
    commit(4, true);
    const bad = commit(5, true);
    git('bisect', 'start', '--first-parent', bad, good);
    // n = 3 consecutive runs always contain one multiple of 3 in the shared counter, so a flaky commit fails every step.
    const out = git('bisect', 'run', 'bash', SCRIPT, '3', join(dir, 't.sh'));
    assert.match(out, new RegExp(`${culprit} is the first '?bad'? commit`));
    git('bisect', 'reset');
    rmSync(counter, { force: true });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

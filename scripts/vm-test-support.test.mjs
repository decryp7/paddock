import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { test } from 'node:test';
import { running } from './vm-test-support.mjs';

// A zombie has ended: only its parent has not collected its exit status yet. In CI's job container PID 1 is `sleep 21600`, which
// never collects, so a detached `slot hold` whose test killed its parent stays a zombie under PID 1 for the job's life, and
// `kill(pid, 0)` still succeeds on it (CI run 8110, job 38513: "the detached `slot hold` ... outlived the test's guard").
test('running: a zombie is not running, although kill(pid, 0) still answers for it', async () => {
  // `sleep 0.1` exits at once and its parent, the shell `exec`'d into `sleep 5`, never waits: a zombie for the next 5 s.
  const parent = spawn('bash', ['-c', 'sleep 0.1 & echo $!; exec sleep 5'], { stdio: ['ignore', 'pipe', 'ignore'] });
  try {
    const pid = Number(await new Promise((r) => parent.stdout.once('data', (d) => r(String(d).trim()))));
    assert.ok(pid > 0);
    let zombie = false;
    for (let i = 0; i < 100 && !zombie; i++) { // conformance: hang-guard — a poll of a condition, 5 s bound only stops a machine that never runs the child
      zombie = spawnSync('ps', ['-o', 'stat=', '-p', String(pid)], { encoding: 'utf8' }).stdout.trim().startsWith('Z');
      if (!zombie) await new Promise((r) => setTimeout(r, 50));
    }
    assert.ok(zombie, `pid ${pid} became a zombie`);
    assert.doesNotThrow(() => process.kill(pid, 0), 'the kernel still answers kill(pid, 0) for a zombie');
    assert.equal(running(pid), false, 'a zombie has ended');
    assert.equal(running(parent.pid), true, 'a live process is running');
  } finally {
    parent.kill('SIGKILL');
  }
});

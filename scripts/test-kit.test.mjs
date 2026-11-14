// The Node test kit (ADR 0103): a wait judged on progress with an adaptive poll, and a reaper that ends every process a test
// started. Each test here was red against the shape it replaces (a fixed 50 ms or 0.2 s poll under one total bound, and a
// test that kills only the process it spawned).
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { test } from 'node:test';
import { Reaper, nextPause, processTable, waitProgressing } from './test-kit.mjs';

const unix = process.platform === 'win32' ? { skip: 'process groups and ps: Unix only; Windows tests end children through their job (not measured here)' } : {};

test('the poll backs off from fast to a cap instead of a fixed pace', () => {
  const pauses = [];
  let p = 0;
  for (let i = 0; i < 12; i++) pauses.push((p = nextPause(p)));
  assert.equal(pauses[0], 1);
  assert.ok(pauses.every((x, i) => i === 0 || x >= pauses[i - 1]), `never shorter: ${pauses}`);
  assert.equal(pauses.at(-1), 100, `capped: ${pauses}`);
});

test('a quick condition is answered quickly and a slower one is not looked at thousands of times', async () => {
  let began = Date.now();
  await waitProgressing('a quick condition', () => Date.now() - began >= 5, { noProgressMs: 5000 });
  assert.ok(Date.now() - began < 60, `answered late: ${Date.now() - began} ms (a fixed 0.2 s poll answers at 200 ms)`);
  began = Date.now();
  let looks = 0;
  await waitProgressing('a slower condition', () => { looks++; return Date.now() - began >= 600; }, { noProgressMs: 5000 });
  assert.ok(looks < 30, `${looks} looks in 600 ms: a fixed fast pace, not a backoff`);
});

test('a wait that keeps moving outlasts its no-progress bound, and one that stops fails naming its level', async () => {
  const began = Date.now();
  await waitProgressing('a moving level', () => Date.now() - began >= 600, { noProgressMs: 150, progress: () => Math.floor((Date.now() - began) / 20) });
  assert.ok(Date.now() - began >= 600);
  const b2 = Date.now();
  await assert.rejects(
    waitProgressing('a stuck level', () => false, { noProgressMs: 150, progress: () => Math.min(15, Math.floor((Date.now() - b2) / 20)) }),
    /a stuck level made no progress for 150 ms: it stood at 15/,
  );
  assert.ok(Date.now() - b2 >= 300 + 150 - 20, `failed at ${Date.now() - b2} ms: a total bound from the start, not a stall`);
});

/** A shell that starts `sleep` in its own process group (job control), detached from its output, prints its pid and waits for a line. */
function parentOfADetachedGrandchild(tag) {
  return ['sh', ['-c', 'set -m; sh -c "sleep 300; exit 0" "$0" </dev/null >/dev/null 2>&1 & echo $!; read _', tag]];
}
const alive = (pid) => { try { process.kill(pid, 0); return true; } catch (e) { return e.code === 'EPERM'; } };
async function firstLine(child) {
  let text = '';
  for await (const chunk of child.stdout) { text += chunk; if (text.includes('\n')) break; }
  return Number(text.trim());
}

test('a detached grandchild whose parent ended is ended with the test', unix, async () => {
  const tag = `paddock-kit-${process.pid}-${Date.now()}`;
  const reaper = new Reaper('the grandchild test');
  const [cmd, args] = parentOfADetachedGrandchild(tag);
  const child = reaper.spawn(cmd, args, { stdio: ['pipe', 'pipe', 'ignore'] });
  const g = await firstLine(child);
  reaper.observe();
  child.stdin.end('\n');
  await once(child, 'exit');
  assert.ok(alive(g), 'the grandchild outlives its parent (the leak this guards)');
  const ended = await reaper.reap();
  assert.ok(!alive(g), `the detached grandchild ${g} outlived the reaper`);
  assert.ok(ended.some((e) => e.pid === g && e.how === 'below a child'), JSON.stringify(ended));
});

test('a process that left before it was seen is found by the marker', unix, async () => {
  const tag = `paddock-kit-${process.pid}-${Date.now()}-m`;
  const [cmd, args] = parentOfADetachedGrandchild(tag);
  const child = spawn(cmd, args, { stdio: ['pipe', 'pipe', 'ignore'] });
  const g = await firstLine(child);
  child.stdin.end('\n');
  await once(child, 'exit');
  const ended = await new Reaper('the marker test', { marker: tag }).reap();
  assert.ok(!alive(g), `the grandchild ${g} that names the marker outlived the reaper`);
  assert.ok(ended.some((e) => e.pid === g && e.how === 'names the marker'), JSON.stringify(ended));
});

test('a pid handed on is never ended: identity is the pid and its start time (rule 53)', unix, () => {
  const me = processTable().find((p) => p.pid === process.pid);
  assert.ok(me && me.started, 'the table reads this process with its start time');
  const reaper = new Reaper('the identity test');
  reaper.adoptIdentity({ pid: process.pid, started: `${me.started} (not this one)` });
  assert.deepEqual(reaper.stillRunning(), [], 'a pid whose start time differs is another process');
});

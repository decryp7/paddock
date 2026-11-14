// The Node test kit (ADR 0103). The schedule of the wait is tested with an injected clock (no wall-clock bound); the reaper's tests
// signal nothing themselves except children they spawned (through their stdin), and the grandchildren they make sleep 20 s and end
// by themselves, so a red run leaks a sleep for at most 20 s and ends no other process.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { FIRST_MS, LAST_MS, Reaper, TOKEN_VAR, choose, nextPause, processTable, sameLevel, waitProgressing } from './test-kit.mjs';

const unix = process.platform === 'win32' ? { skip: 'the reaper and the process table are Unix only (ps); not measured on Windows' } : {};

/** A fake clock and sleep: time moves only when the wait sleeps. */
function fakeTime() {
  const t = { at: 0, pauses: [] };
  t.now = () => t.at;
  t.sleep = async (ms) => { t.pauses.push(ms); t.at += ms; };
  return t;
}

test('the poll backs off from FIRST_MS doubling to LAST_MS instead of a fixed pace', () => {
  const pauses = [];
  let p = 0;
  for (let i = 0; i < 12; i++) pauses.push((p = nextPause(p)));
  assert.equal(pauses[0], FIRST_MS);
  for (let i = 1; i < pauses.length; i++) assert.equal(pauses[i], Math.min(pauses[i - 1] * 2, LAST_MS), `${pauses}`);
  assert.equal(pauses.at(-1), LAST_MS);
});

test('a wait sleeps the backoff schedule between looks and returns the first truthy value', async () => {
  const t = fakeTime();
  let looks = 0;
  const v = await waitProgressing('a condition', () => (++looks === 6 ? 'done' : false), { noProgressMs: 10_000, now: t.now, sleep: t.sleep });
  assert.equal(v, 'done');
  assert.deepEqual(t.pauses, [1, 2, 4, 8, 16]);
});

test('a wait that keeps moving outlasts its no-progress bound, and one that stops fails at the bound after the stall', async () => {
  let t = fakeTime();
  // Moves every 20 ms of fake time for 600 ms: four times the 150 ms bound in total, never stalled.
  await waitProgressing('a moving level', () => t.at >= 600, { noProgressMs: 150, progress: () => Math.floor(t.at / 20), now: t.now, sleep: t.sleep });
  assert.ok(t.at >= 600);
  t = fakeTime();
  await assert.rejects(
    waitProgressing('a stuck level', () => false, { noProgressMs: 150, progress: () => Math.min(15, Math.floor(t.at / 20)), now: t.now, sleep: t.sleep }),
    /a stuck level made no progress for 150 ms: it stood at 15/,
  );
  // The level reached 15 at 300 ms of fake time: a total bound from the start would have failed at 150.
  assert.ok(t.at >= 300 + 150, `failed at ${t.at} ms: a total bound from the start, not a stall`);
});

test('progress levels compare by value: an equal object is no progress, a changed one is', () => {
  assert.ok(sameLevel(3, 3) && sameLevel(NaN, NaN) && sameLevel({ a: [1] }, { a: [1] }));
  assert.ok(!sameLevel({ a: 1 }, { a: 2 }) && !sameLevel(1, '1') && !sameLevel(null, {}));
});

test('the process table fails closed: a ps that cannot run throws instead of listing nothing', unix, () => {
  const path = process.env.PATH;
  process.env.PATH = '/nonexistent-paddock-kit';
  try {
    assert.throws(() => processTable(), /ps failed/);
  } finally {
    process.env.PATH = path;
  }
  const me = processTable().find((p) => p.pid === process.pid);
  assert.ok(me && Number.isFinite(me.started) && me.uid === process.getuid(), JSON.stringify(me));
});

test('the rule chooses only what it can prove: children, their descendants, its groups, its token, orphans naming its folder', () => {
  const created = Date.parse('Sun Nov 15 01:00:00 2026');
  const row = (pid, over = {}) => ({ pid, ppid: 500, pgid: pid, uid: 501, started: created + 1000, lstart: `L${pid}`, command: 'x', ...over });
  const facts = { roots: [{ pid: 10, lstart: 'L10' }], below: [{ pid: 11, lstart: 'L11' }], groups: new Map([[10, { pid: 10, lstart: 'L10' }]]), created, uid: 501, me: 42, token: 't', folders: ['/s/reaper-t-0'], carry: (pid) => pid === 13 };
  const table = [
    row(10), row(11), row(12, { pgid: 10 }), row(13), row(14, { ppid: 1, command: 'sh /s/reaper-t-0' }),
    row(15, { command: 'sh /s/reaper-t-0' }), // names the folder with a live parent: not proven
    row(16, { pgid: 10, started: created - 5000 }), // in the group but started before the reaper
    row(17, { pgid: 10, uid: 0 }), // another user
    row(42, { pgid: 10 }), // this process
    row(19, { pgid: 77 }), // nothing ties it to the reaper
  ];
  assert.deepEqual(choose(table, facts).map((c) => `${c.pid}:${c.how}`), [
    '10:child', '11:below a child', '12:in a group the reaper made', "13:carries the reaper's token", '14:names a folder the reaper made',
  ]);
  // A pid handed on (the same pid, another start time) is not the child.
  assert.deepEqual(choose([row(10, { lstart: 'other', pgid: 99 })], facts), []);
});

test('a group counts while its leader is the process seen, or while no process holds its pid', () => {
  const created = Date.parse('Sun Nov 15 01:00:00 2026');
  const row = (pid, over = {}) => ({ pid, ppid: 500, pgid: 10, uid: 501, started: created + 1000, lstart: `L${pid}`, command: 'x', ...over });
  const facts = { roots: [], below: [], groups: new Map([[10, { pid: 10, lstart: 'L10' }]]), created, uid: 501, me: 42, token: 't', folders: [], carry: () => false };
  const pids = (table) => choose(table, facts).map((c) => c.pid);
  assert.deepEqual(pids([row(10), row(12)]), [10, 12]);
  assert.deepEqual(pids([row(12)]), [12], 'the leader is gone; the group keeps its id while it has members');
  assert.deepEqual(pids([row(10, { lstart: 'another' }), row(12)]), [], 'pid 10 is another process: the group id may be someone else\'s');
});

/** A shell that starts `sleep 20` under a second shell whose $0 is `arg`, in a group of its own (set -m), prints its pid, then waits for a line. */
function parentOfADetachedGrandchild(arg) {
  return ['sh', ['-c', 'set -m; sh -c "sleep 20; exit 0" "$0" </dev/null >/dev/null 2>&1 & echo $!; read _', arg]];
}
const runningAs = (p) => processTable().some((t) => t.pid === p.pid && t.lstart === p.lstart);
async function firstLine(child) {
  let text = '';
  for await (const chunk of child.stdout) { text += chunk; if (text.includes('\n')) break; }
  const pid = Number(text.trim());
  return processTable().find((t) => t.pid === pid);
}

test('a detached grandchild whose parent ended is ended with the test', unix, async () => {
  const reaper = new Reaper('the grandchild test');
  const [cmd, args] = parentOfADetachedGrandchild('x');
  const child = reaper.spawn(cmd, args, { stdio: ['pipe', 'pipe', 'ignore'] });
  const exited = once(child, 'exit');
  const g = await firstLine(child);
  reaper.observe();
  child.stdin.end('\n');
  await exited;
  assert.ok(runningAs(g), 'the grandchild outlives its parent (the leak this guards)');
  const ended = await reaper.reap();
  assert.ok(!runningAs(g), `the detached grandchild ${g.pid} outlived the reaper`);
  assert.ok(ended.some((e) => e.pid === g.pid), JSON.stringify(ended));
});

test('an orphan naming the reaper\'s folder is ended, and a process that only names it is not', unix, async () => {
  const parent = mkdtempSync(join(process.env.PADDOCK_TEST_SCRATCH ?? tmpdir(), 'kit-'));
  try {
    const reaper = new Reaper('the folder test');
    const folder = reaper.scratchIn(parent);
    const [cmd, args] = parentOfADetachedGrandchild(folder);
    const outside = spawn(cmd, args, { stdio: ['pipe', 'pipe', 'ignore'] });
    const orphan = await firstLine(outside);
    outside.stdin.end('\n');
    await once(outside, 'exit');
    // A child of this test (not the reaper's) whose child names the folder and has a live parent: naming it proves nothing.
    const holder = spawn('sh', ['-c', 'exec 3<&0; sh -c "read _ <&3; exit 0" "$F" & echo $!; wait'], { env: { ...process.env, F: folder }, stdio: ['pipe', 'pipe', 'ignore'] });
    const exited = once(holder, 'exit');
    try {
      const held = await firstLine(holder);
      const chosen = reaper.candidates();
      assert.ok(chosen.some((c) => c.pid === orphan.pid && c.how === 'names a folder the reaper made'), JSON.stringify(chosen));
      assert.ok(!chosen.some((c) => c.pid === held.pid), `a process with a live parent and no token: ${JSON.stringify(chosen)}`);
      await reaper.reap();
      assert.ok(!runningAs(orphan), `the orphan ${orphan.pid} outlived the reaper`);
      assert.ok(runningAs(held), 'the reaper ended a process it could not prove was the test\'s');
    } finally {
      // The holder ends when its stdin closes, also when an assertion above failed (a red run must not hang the file).
      holder.stdin.end();
      await exited;
    }
  } finally {
    rmSync(parent, { recursive: true, force: true });
  }
});

test('another reaper\'s child is left alone, and the token is in its environment', unix, async () => {
  const a = new Reaper('reaper a');
  const b = new Reaper('reaper b');
  const child = b.spawn('sh', ['-c', `echo "$${TOKEN_VAR}"; read _`], { stdio: ['pipe', 'pipe', 'ignore'] });
  const exited = once(child, 'exit');
  try {
    let text = '';
    for await (const chunk of child.stdout) { text += chunk; if (text.includes('\n')) break; }
    assert.equal(text.trim(), b.token);
    assert.ok(!a.candidates().some((c) => c.pid === child.pid));
    await a.reap();
    assert.ok(processTable().some((t) => t.pid === child.pid), 'reaper a ended reaper b\'s child');
  } finally {
    child.stdin.end('\n');
    await exited;
  }
  assert.deepEqual(await b.reap(), []);
});

test('a runner that kills the test\'s group also ends the reaper\'s children', unix, async () => {
  // The fake runner is this test: it starts a "test" (a node process) leading a group of its own, as a runner's timeout kill
  // expects, kills that group (its own child's, nothing else) and checks that the reaper's `sleep 60` ended with it, although the
  // reaper never got to reap. Safe while red: a child that escaped is a sleep that ends by itself.
  const kit = new URL('./test-kit.mjs', import.meta.url).href;
  const script = `import { Reaper } from ${JSON.stringify(kit)};
const r = new Reaper('under a fake runner');
const c = r.spawn('sleep', ['60'], { stdio: 'ignore' });
console.log('pid ' + c.pid);
setInterval(() => {}, 1000);`;
  const test = spawn(process.execPath, ['--input-type=module', '-e', script], { stdio: ['ignore', 'pipe', 'ignore'], detached: true });
  const exited = once(test, 'exit');
  let text = '';
  for await (const chunk of test.stdout) { text += chunk; if (/pid \d+\n/.test(text)) break; }
  const pid = Number(/pid (\d+)/.exec(text)[1]);
  const sleep = processTable().find((t) => t.pid === pid);
  assert.ok(sleep, 'the reaper\'s child runs');
  process.kill(-test.pid, 'SIGKILL'); // the test's group: this test's own child leads it
  await exited;
  await waitProgressing('the reaper\'s child to end with the test\'s group', () => !runningAs(sleep), { noProgressMs: 30_000 }).catch(() => {});
  assert.ok(!runningAs(sleep), `the reaper's child ${pid} outlived a runner that killed the test's group`);
});

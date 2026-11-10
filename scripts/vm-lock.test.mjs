// scripts/vm-lock.sh: the interim exclusive lock for the Windows test VM. Uses the real `./dev ci slot` (an OS file lock) under a
// scratch root; no VM, no vmrun: the "VM script" is a stub that only logs what it would do.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { warmDev } from './vm-test-support.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const LIB = join(here, 'vm-lock.sh');
const DEV = join(here, '..', 'dev');
const scratch = mkdtempSync(join(process.env.PADDOCK_TEST_SCRATCH ?? tmpdir(), 'vm-lock-test-'));
after(() => rmSync(scratch, { recursive: true, force: true }));
before(warmDev);

/** A holder's body: it waits until its process group is killed, never for a fixed time (a slow machine must not outlast it, ER-12). */
const FOREVER = 'while :; do sleep 0.2; done';
let n = 0;
/** Kills the holder's whole process group (its sleep too) so nothing outlives the test. */
const killGroup = (child) => { try { process.kill(-child.pid, 'SIGKILL'); } catch { /* gone */ } };
const fresh = () => {
  const root = join(scratch, `root${++n}`);
  const tmp = join(scratch, `tmp${n}`); // the owner folders of killed holders live here and go with the scratch folder (their holders end with them)
  mkdirSync(tmp, { recursive: true });
  return { root, log: join(scratch, `log${n}`), env: { ...process.env, TMPDIR: tmp, PADDOCK_VM_LOCK_ROOT: root, PADDOCK_DEV: DEV, PADDOCK_VM_LOCK_HELD: '' } };
};

/** A stub "VM script": takes the lock as `label`, logs, runs `body` (bash), and logs the end. */
const stub = (label, log, body = 'true') =>
  `. '${LIB}'; vm_lock_acquire ${label}; echo "${label} start" >> '${log}'; ${body}; echo "${label} end" >> '${log}'`;
const bash = (script, env) => spawnSync('bash', ['-c', script], { env, encoding: 'utf8', timeout: 60_000 });
const lines = (f) => (existsSync(f) ? readFileSync(f, 'utf8').trim().split('\n') : []);
/** Polls `cond` every 100 ms until it holds; the bound only stops a hung child, so the verdict never depends on the time taken. */
const until = async (cond) => {
  for (let i = 0; i < 150 && !cond(); i++) await new Promise((r) => setTimeout(r, 100)); // conformance: hang-guard — a poll of a condition, 15 s bound that only stops a hung child
  return cond();
};

test('a second acquirer fails at once with ::error naming the holder (pid, start time) and the next step', async () => {
  const t = fresh();
  const holder = spawn('bash', ['-c', stub('first', t.log, `echo ready >> '${t.log}'; ${FOREVER}`)], { env: t.env, stdio: 'ignore', detached: true });
  try {
    await until(() => lines(t.log).includes('ready'));
    assert.ok(lines(t.log).includes('ready'), 'the first holder got the lock');
    const second = bash(stub('second', t.log), t.env);
    assert.notEqual(second.status, 0);
    assert.match(second.stderr, /::error::.*test machine.*held by first/);
    assert.match(second.stderr, new RegExp(`pid ${holder.pid}\\b`));
    assert.match(second.stderr, /started [A-Z][a-z]{2} (\d|[A-Z][a-z]{2} +\d)/);
    assert.match(second.stderr, /next step/i);
    assert.ok(!lines(t.log).includes('second start'), 'the second never ran');
  } finally {
    killGroup(holder);
  }
});

test('the lock is released on exit, also when the script fails', () => {
  const t = fresh();
  const failed = bash(stub('a', t.log, 'exit 7'), t.env);
  assert.equal(failed.status, 7, 'the script keeps its own exit code');
  const next = bash(stub('b', t.log), t.env);
  assert.equal(next.status, 0, next.stderr);
  assert.deepEqual(lines(t.log), ['a start', 'b start', 'b end']);
});

test('with PADDOCK_VM_LOCK_WAIT a second acquirer waits for the first and then runs, never together', async () => {
  const t = fresh();
  const first = spawn('bash', ['-c', stub('first', t.log, 'sleep 2')], { env: t.env, stdio: 'ignore', detached: true });
  await until(() => lines(t.log).includes('first start'));
  const second = bash(stub('second', t.log), { ...t.env, PADDOCK_VM_LOCK_WAIT: '30', PADDOCK_VM_LOCK_POLL: '1' });
  assert.equal(second.status, 0, second.stderr);
  await new Promise((r) => first.on('exit', r));
  assert.deepEqual(lines(t.log), ['first start', 'first end', 'second start', 'second end']);
});

test('the wait is bounded: after PADDOCK_VM_LOCK_WAIT seconds it fails with ::error', async () => {
  const t = fresh();
  const holder = spawn('bash', ['-c', stub('first', t.log, FOREVER)], { env: t.env, stdio: 'ignore', detached: true });
  try {
    await until(() => lines(t.log).includes('first start'));
    const second = bash(stub('second', t.log), { ...t.env, PADDOCK_VM_LOCK_WAIT: '2', PADDOCK_VM_LOCK_POLL: '1' });
    assert.notEqual(second.status, 0);
    assert.match(second.stderr, /::error::.*after 2s/);
  } finally {
    killGroup(holder);
  }
});

test('a holder that was killed (no trap ran) frees the lock: its pid is gone, so the next acquirer takes it', async () => {
  const t = fresh();
  const holder = spawn('bash', ['-c', stub('dead', t.log, FOREVER)], { env: t.env, stdio: 'ignore', detached: true });
  await until(() => lines(t.log).includes('dead start'));
  killGroup(holder);
  await new Promise((r) => holder.on('exit', r));
  const next = bash(stub('alive', t.log), { ...t.env, PADDOCK_VM_LOCK_WAIT: '30', PADDOCK_VM_LOCK_POLL: '1' });
  assert.equal(next.status, 0, next.stderr);
  assert.match(next.stderr, /dead holder|no longer running/i);
  assert.ok(lines(t.log).includes('alive end'));
});

test('a reused pid does not keep a dead holder alive (rule 53: identity is pid plus start time)', () => {
  const t = fresh();
  // Hold the slot by hand with an owner folder whose recorded pid is alive (ours) but whose start time is wrong.
  const owner = mkdtempSync(join(scratch, 'owner-'));
  writeFileSync(join(owner, 'who'), `${process.pid}\tThu Jan  1 00:00:00 1970\tstale\n`);
  const held = spawnSync('bash', ['-c', `'${DEV}' ci slot acquire --max 1 --owner '${owner}' --root '${t.root}'`], { encoding: 'utf8' });
  assert.equal(held.status, 0, held.stderr);
  const next = bash(stub('fresh', t.log), { ...t.env, PADDOCK_VM_LOCK_WAIT: '30', PADDOCK_VM_LOCK_POLL: '1' });
  assert.equal(next.status, 0, next.stderr);
});

test('a script run under a holder that already took the lock (chain script calling vm-win.sh) does not wait for itself', () => {
  const t = fresh();
  const inner = `bash -c ". '${LIB}'; vm_lock_acquire inner; echo inner >> '${t.log}'"`;
  const r = bash(stub('outer', t.log, inner), t.env);
  assert.equal(r.status, 0, r.stderr);
  assert.deepEqual(lines(t.log), ['outer start', 'inner', 'outer end']);
});

// --- review fixes (P1-1 to P1-4 and the P2s) ----------------------------------------------------------------------------------
const RUN = join(here, 'vm-lock-run.sh');
const waitFor = async (f, line) => { assert.ok(await until(() => lines(f).includes(line)), `${line} in ${f}`); };
/** A holder in its own process group (killed with the group, so no orphaned sleep or detached holder outlives the test). */
const hold = async (t, label, body = FOREVER) => {
  const child = spawn('bash', ['-c', stub(label, t.log, `echo ready-${label} >> '${t.log}'; ${body}`)], { env: t.env, stdio: 'ignore', detached: true });
  await waitFor(t.log, `ready-${label}`);
  const exited = new Promise((r) => child.on('exit', r));
  return { child, stop: async () => { try { process.kill(-child.pid, 'SIGKILL'); } catch { /* gone */ } await exited; } };
};
/** The start the lock records for a pid (vm-lock.sh's own function: `ps lstart`, plus the kernel start tick on Linux). */
const lstart = (pid) => spawnSync('bash', ['-c', `. '${LIB}'; _vm_lock_start ${Number(pid)}`], { encoding: 'utf8' }).stdout.trim();

test('P1-1: the hold has no time cap: vm-lock.sh passes --max-hold-secs 0, so the default cap paddock-dev would apply never ends it', () => {
  const t = fresh();
  // A paddock-dev stub that logs the arguments of every acquire (the cap is paddock-dev's, enforced in real time; what vm-lock.sh owes
  // it is the explicit "no cap" argument, which is what this checks, without waiting for any time to pass).
  const dev = join(scratch, 'capped-dev.sh');
  const argsLog = join(scratch, `acquire-args-${n}`);
  writeFileSync(dev, `#!/bin/bash\nif [ "$3" = acquire ]; then echo "$*" >> '${argsLog}'; fi\nexec '${DEV}' "$@"\n`, { mode: 0o755 });
  const r = bash(stub('capless', t.log), { ...t.env, PADDOCK_DEV: dev });
  assert.equal(r.status, 0, r.stderr);
  const acquires = lines(argsLog);
  assert.equal(acquires.length, 1, 'one acquire');
  assert.match(acquires[0], /(^| )--max-hold-secs 0( |$)/, `the acquire passes the no-cap argument: ${acquires[0]}`);
});

test('P1-2: PADDOCK_VM_LOCK_HELD is accepted only for a live holder with that pid, start time and owner folder', async () => {
  const t = fresh();
  const h = await hold(t, 'real');
  try {
    const owner = readFileSync(join(t.root, 'slots', 's1.owner'), 'utf8').match(/owner=(.*)/)?.[1];
    assert.ok(owner, 'the slot names its owner folder');
    const good = `${h.child.pid}\t${lstart(h.child.pid)}\t${owner}`;
    const cases = {
      'a stale pid': `999999\tMon 1 Jan 00:00:00 2001\t${owner}`,
      'an unrelated live pid ($PPID)': `${process.pid}\t${lstart(process.pid)}\t${owner}`,
      'the holder pid with the wrong start time': `${h.child.pid}\tMon 1 Jan 00:00:00 2001\t${owner}`,
      'the holder pid but an owner that no longer holds': `${h.child.pid}\t${lstart(h.child.pid)}\t${owner}-gone`,
      'a bare pid': String(h.child.pid),
    };
    for (const [what, held] of Object.entries(cases)) {
      const r = bash(stub('intruder', t.log), { ...t.env, PADDOCK_VM_LOCK_HELD: held });
      assert.equal(r.status, 75, `${what} must not bypass the lock: ${r.stderr}`);
    }
    assert.ok(!lines(t.log).includes('intruder start'));
    const ok = bash(stub('child', t.log), { ...t.env, PADDOCK_VM_LOCK_HELD: good });
    assert.equal(ok.status, 0, `the real holder's value is accepted: ${ok.stderr}`);
  } finally { await h.stop(); }
});

test('P2: busy exits 75 and hides paddock-dev\'s "raise --max" text; release logs label and duration', async () => {
  const t = fresh();
  const h = await hold(t, 'first');
  try {
    const r = bash(stub('second', t.log), t.env);
    assert.equal(r.status, 75);
    assert.doesNotMatch(r.stderr, /raise --max/);
  } finally { await h.stop(); }
  const ok = bash(stub('timed', t.log), t.env);
  assert.match(ok.stderr, /vm-lock: timed released the test machines lock after \d+s/);
});

test('P2: an EXIT trap set before the acquire still runs (quotes intact) and the lock is released; the exit code is kept', () => {
  const t = fresh();
  const script = `trap 'echo "mine: it'"'"'s \\"quoted\\"" >> '${t.log}'' EXIT; . '${LIB}'; vm_lock_acquire chained; exit 5`;
  const r = bash(script, t.env);
  assert.equal(r.status, 5);
  assert.deepEqual(lines(t.log), ['mine: it\'s "quoted"']);
  assert.equal(bash(stub('after', t.log), t.env).status, 0, 'released');
});

test('an EXIT trap set before the acquire runs while the lock is still held (cleanup of a VM must not happen unlocked); the lock is released after it', () => {
  const t = fresh();
  const seen = join(scratch, `trap-status-${n}`);
  const script = `trap '"${DEV}" ci slot status --max 1 --root "${t.root}" --json > "${seen}"; echo "$_vm_lock_owner" >> "${seen}.owner"' EXIT; . '${LIB}'; vm_lock_acquire ordered; exit 0`;
  const r = bash(script, t.env);
  assert.equal(r.status, 0, r.stderr);
  const status = JSON.parse(readFileSync(seen, 'utf8'));
  const owner = readFileSync(`${seen}.owner`, 'utf8').trim();
  assert.ok(owner, 'the lock had an owner folder when the earlier trap ran');
  const slot = status.slots.find((x) => x.busy);
  assert.ok(slot, `a slot was busy inside the earlier trap: ${JSON.stringify(status)}`);
  assert.ok(JSON.stringify(slot).includes(owner), `busy with this holder's owner folder: ${JSON.stringify(slot)}`);
  assert.equal(bash(stub('after', t.log), t.env).status, 0, 'released afterwards');
});

test('P1-4: vm-lock-run.sh holds the lock around a command, passes its exit code, and a second run fails before its command starts', async () => {
  const t = fresh();
  const ran = bash(`'${RUN}' viarun -- bash -c "echo inner >> '${t.log}'; exit 9"`, t.env);
  assert.equal(ran.status, 9);
  const h = spawn('bash', [RUN, 'long', '--', 'bash', '-c', `echo ready >> '${t.log}'; ${FOREVER}`], { env: t.env, stdio: 'ignore', detached: true });
  try {
    await waitFor(t.log, 'ready');
    const second = bash(`'${RUN}' second -- bash -c "echo SECOND >> '${t.log}'"`, t.env);
    assert.equal(second.status, 75);
    assert.match(second.stderr, /::error::.*held by long/);
    assert.ok(!lines(t.log).includes('SECOND'));
    assert.equal(bash(`'${RUN}' --check`, t.env).status, 1, 'no inherited lock');
  } finally { try { process.kill(-h.pid, 'SIGKILL'); } catch { /* gone */ } }
});

test('P2: a lock that frees between our failed acquire and the status is retried, not reported as an unknown holder', () => {
  const t = fresh();
  const dev = join(scratch, 'racy-dev.sh'); // the first acquire answers "busy", status then shows the slot free (the holder just ended)
  const flag = join(scratch, `racy-${n}`);
  writeFileSync(dev, `#!/bin/bash\nif [ "$3" = acquire ] && [ ! -e '${flag}' ]; then touch '${flag}'; echo busy >&2; exit 4; fi\nif [ "$3" = status ]; then echo '{"ok":true,"slots":[{"slot":"s1","busy":false,"pid":null,"owner":null}]}'; exit 0; fi\nexec '${DEV}' "$@"\n`, { mode: 0o755 });
  const r = bash(stub('retrier', t.log), { ...t.env, PADDOCK_DEV: dev });
  assert.equal(r.status, 0, r.stderr);
  assert.doesNotMatch(r.stderr, /unknown \(no who file\)/);
});

test('a holder is the same process when its start token matches, even if `ps lstart` drifts by a second (Linux computes it from the clock)', () => {
  const same = (a, b) => spawnSync('bash', ['-c', `. '${LIB}'; _vm_lock_same_start "$1" "$2"`, 'x', a, b]).status === 0;
  assert.ok(same('Mon Nov 9 10:47:00 2026 [t4242]', 'Mon Nov 9 10:47:01 2026 [t4242]'), 'a drifting lstart with the same kernel start tick');
  assert.ok(!same('Mon Nov 9 10:47:00 2026 [t4242]', 'Mon Nov 9 10:47:00 2026 [t4243]'), 'another start tick is another process');
  assert.ok(same('Mon 9 Nov 17:27:42 2026', 'Mon 9 Nov 17:27:42 2026'), 'no token (macOS): the whole string');
  assert.ok(!same('Mon 9 Nov 17:27:42 2026', 'Mon 9 Nov 17:27:43 2026'), 'no token: a different string is a different process');
  assert.ok(same('Mon Nov 9 10:47:00 2026 [t4242]', 'Mon Nov 9 10:47:00 2026'), 'a token on one side only: the lstart parts decide, a live lock is not stolen');
  assert.ok(!same('Mon Nov 9 10:47:00 2026 [t4242]', 'Tue Nov 10 10:47:00 2026'), 'a token on one side only, and a different lstart: another process');
  assert.ok(!same('', ''), 'an empty start is never a match');
});

test('no holder in the lock tests lives for a fixed time: a holder waits for its kill, so a slow machine cannot outlast it (ER-12)', () => {
  for (const file of ['vm-lock.test.mjs', 'vm-win.test.mjs']) {
    const longSleeps = readFileSync(join(here, file), 'utf8').split('\n').filter((l) => /sleep (\d{2,}|[3-9])\b/.test(l) && !l.includes('longSleeps'));
    assert.deepEqual(longSleeps, [], `${file}: a holder that sleeps a fixed time frees the lock when the test runs longer`);
  }
});

// scripts/vm-lock.sh: the interim exclusive lock for the Windows test VM. Uses the real `./dev ci slot` (an OS file lock) under a
// scratch root; no VM, no vmrun: the "VM script" is a stub that only logs what it would do.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { after, test } from 'node:test';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const LIB = join(here, 'vm-lock.sh');
const DEV = join(here, '..', 'dev');
const scratch = mkdtempSync(join(process.env.PADDOCK_TEST_SCRATCH ?? tmpdir(), 'vm-lock-test-'));
after(() => rmSync(scratch, { recursive: true, force: true }));

let n = 0;
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

test('a second acquirer fails at once with ::error naming the holder (pid, start time) and the next step', async () => {
  const t = fresh();
  const holder = spawn('bash', ['-c', stub('first', t.log, `echo ready >> '${t.log}'; sleep 20`)], { env: t.env, stdio: 'ignore' });
  try {
    for (let i = 0; i < 100 && !lines(t.log).includes('ready'); i++) await new Promise((r) => setTimeout(r, 100));
    assert.ok(lines(t.log).includes('ready'), 'the first holder got the lock');
    const second = bash(stub('second', t.log), t.env);
    assert.notEqual(second.status, 0);
    assert.match(second.stderr, /::error::.*test machine.*held by first/);
    assert.match(second.stderr, new RegExp(`pid ${holder.pid}\\b`));
    assert.match(second.stderr, /started [A-Z][a-z]{2} \d/);
    assert.match(second.stderr, /next step/i);
    assert.ok(!lines(t.log).includes('second start'), 'the second never ran');
  } finally {
    holder.kill('SIGKILL');
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
  const first = spawn('bash', ['-c', stub('first', t.log, 'sleep 2')], { env: t.env, stdio: 'ignore' });
  for (let i = 0; i < 100 && !lines(t.log).includes('first start'); i++) await new Promise((r) => setTimeout(r, 50));
  const second = bash(stub('second', t.log), { ...t.env, PADDOCK_VM_LOCK_WAIT: '30', PADDOCK_VM_LOCK_POLL: '1' });
  assert.equal(second.status, 0, second.stderr);
  await new Promise((r) => first.on('exit', r));
  assert.deepEqual(lines(t.log), ['first start', 'first end', 'second start', 'second end']);
});

test('the wait is bounded: after PADDOCK_VM_LOCK_WAIT seconds it fails with ::error', async () => {
  const t = fresh();
  const holder = spawn('bash', ['-c', stub('first', t.log, 'sleep 20')], { env: t.env, stdio: 'ignore' });
  try {
    for (let i = 0; i < 100 && !lines(t.log).includes('first start'); i++) await new Promise((r) => setTimeout(r, 50));
    const second = bash(stub('second', t.log), { ...t.env, PADDOCK_VM_LOCK_WAIT: '2', PADDOCK_VM_LOCK_POLL: '1' });
    assert.notEqual(second.status, 0);
    assert.match(second.stderr, /::error::.*after 2s/);
  } finally {
    holder.kill('SIGKILL');
  }
});

test('a holder that was killed (no trap ran) frees the lock: its pid is gone, so the next acquirer takes it', async () => {
  const t = fresh();
  const holder = spawn('bash', ['-c', stub('dead', t.log, 'sleep 60')], { env: t.env, stdio: 'ignore' });
  for (let i = 0; i < 100 && !lines(t.log).includes('dead start'); i++) await new Promise((r) => setTimeout(r, 50));
  holder.kill('SIGKILL');
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

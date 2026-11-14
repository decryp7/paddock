// The Node test kit (ADR 0103), for scripts/*.test.mjs and conformance/test. Not a test file: `node --test` runs only *.test.mjs.
//
// - waitProgressing(what, done, { noProgressMs, progress }): waits until `done()` is true, polling with a backoff (nextPause:
//   1 ms doubling to 100 ms, the old tests' fixed pace as the worst case), and fails only when `progress()` (a level: lines written, files made; any change counts) has not
//   changed for `noProgressMs`. Without `progress` the bound is a plain hang guard from the start. A test that runs several steps
//   waits on each step's progress, never on one total summed over the steps (runs 8060-8099).
// - Reaper: every process a test started ends with the test (ER-14). It keeps the identity (pid and start time, rule 53) of each
//   child it spawned and of every process it has seen below one (a sampler looks at the process table with the same backoff), and
//   with a `marker` also ends every process whose command line names it (the test's scratch folder). reap() ends them (the process
//   group first when the process leads one, then the process), waits until they are gone, and returns what it had to end.
//   The Rust counterpart is paddock_testkit_start::Reaper, which ends processes through paddock_proc.
import { spawn, spawnSync } from 'node:child_process';

/** The first and the longest pause of a poll, in ms. */
export const FIRST_MS = 1;
export const LAST_MS = 100;
/** How long reap() waits for what it ended to be gone: a bound that only stops a process the OS does not end. */
export const GONE_WITHIN_MS = 15_000;
/** The default no-progress bound: a hang guard under node --test's and CI's own bounds. */
export const HANG_GUARD_MS = 40_000;

/** The pause after `previous` (0 for the first): doubles from FIRST_MS up to LAST_MS. */
export const nextPause = (previous) => (previous <= 0 ? FIRST_MS : Math.min(previous * 2, LAST_MS));

const pause = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * Waits until `done()` (sync or async) is truthy and returns its value. Fails, naming `what`, the level and the time, only when
 * `progress()` has stood still for `noProgressMs` (default HANG_GUARD_MS); without `progress` that bound counts from the start.
 */
export async function waitProgressing(what, done, { noProgressMs = HANG_GUARD_MS, progress = () => 0 } = {}) {
  const began = Date.now();
  let level = await progress();
  let since = began;
  let p = 0;
  for (;;) {
    const v = await done();
    if (v) return v;
    const now = await progress();
    if (now !== level) { level = now; since = Date.now(); }
    if (Date.now() - since >= noProgressMs) {
      throw new Error(`${what} made no progress for ${noProgressMs} ms: it stood at ${level} (after ${Date.now() - began} ms in all; a slow host that keeps moving passes, only a stall fails)`);
    }
    p = nextPause(p);
    await pause(p);
  }
}

/** The process table: [{ pid, ppid, pgid, started, command }] (`ps`, Unix). Empty when it cannot be read. */
export function processTable() {
  if (process.platform === 'win32') return [];
  const r = spawnSync('ps', ['-axww', '-o', 'pid=,ppid=,pgid=,lstart=,command='], { encoding: 'utf8', env: { ...process.env, LC_ALL: 'C' } });
  if (r.status !== 0) return [];
  const rows = [];
  for (const line of r.stdout.split('\n')) {
    // lstart is five words: "Sat Nov 14 21:41:09 2026".
    const m = /^\s*(\d+)\s+(\d+)\s+(\d+)\s+(\S+\s+\S+\s+\S+\s+\S+\s+\S+)\s+(.*)$/.exec(line);
    if (m) rows.push({ pid: Number(m[1]), ppid: Number(m[2]), pgid: Number(m[3]), started: m[4], command: m[5] });
  }
  return rows;
}

const same = (a, b) => a.pid === b.pid && a.started === b.started;

/** The guard for the processes of one test (see the file's header). */
export class Reaper {
  /** @param {string} what a name for its messages; @param {{ marker?: string }} options */
  constructor(what, { marker } = {}) {
    this.what = what;
    this.marker = marker;
    this.roots = [];
    this.below = [];
    this.stopped = false;
    this.pace = 0;
    this.timer = null;
    this.tick();
  }

  tick() {
    if (this.stopped) return;
    this.observe();
    this.pace = nextPause(this.pace);
    this.timer = setTimeout(() => this.tick(), Math.max(this.pace, 20));
    this.timer.unref();
  }

  /** Starts a child (node's spawn) and keeps its identity. */
  spawn(command, args = [], options = {}) {
    const child = spawn(command, args, options);
    if (child.pid) this.adoptPid(child.pid);
    return child;
  }

  /** Keeps the identity of a process the test learned the pid of. */
  adoptPid(pid) {
    const row = processTable().find((p) => p.pid === pid);
    if (row) this.adoptIdentity(row);
  }

  /** Keeps an identity read earlier ({ pid, started }). */
  adoptIdentity(id) {
    if (!this.roots.some((r) => same(r, id))) this.roots.push({ pid: id.pid, started: id.started });
  }

  /** Looks at the process table now and remembers every process below a known one. */
  observe(table = processTable()) {
    const known = [...this.roots, ...this.below].filter((k) => table.some((p) => same(p, k)));
    const seen = new Set(known.map((k) => k.pid));
    let grew = true;
    while (grew) {
      grew = false;
      for (const p of table) {
        if (!seen.has(p.pid) && seen.has(p.ppid)) {
          seen.add(p.pid);
          if (!this.below.some((b) => same(b, p))) this.below.push({ pid: p.pid, started: p.started });
          grew = true;
        }
      }
    }
  }

  /** The known processes still running, each { pid, started, pgid, how }. */
  stillRunning(table = processTable()) {
    const out = [];
    const add = (id, how) => {
      const row = table.find((p) => same(p, id));
      if (row && row.pid !== process.pid && !out.some((o) => o.pid === row.pid)) out.push({ pid: row.pid, started: row.started, pgid: row.pgid, how });
    };
    for (const r of this.roots) add(r, 'child');
    for (const b of this.below) add(b, 'below a child');
    if (this.marker) for (const p of table.filter((t) => t.command.includes(this.marker))) add(p, 'names the marker');
    return out;
  }

  /** Ends everything still running, waits until it is gone (at most GONE_WITHIN_MS) and returns what it had to end. */
  async reap() {
    this.stopped = true;
    clearTimeout(this.timer);
    const table = processTable();
    this.observe(table);
    const todo = this.stillRunning(table);
    for (const p of todo) {
      // The group, when this process leads it (a detached child), then the process; each only while it is still the same one.
      const now = processTable().find((t) => same(t, p));
      if (!now) continue;
      if (now.pgid === now.pid && now.pgid !== process.pid) { try { process.kill(-now.pgid, 'SIGKILL'); } catch { /* gone */ } }
      try { process.kill(now.pid, 'SIGKILL'); } catch { /* gone */ }
      process.stderr.write(`reaper (${this.what}): ended pid ${p.pid} (${p.how}), still running at the end of the test\n`);
    }
    try {
      await waitProgressing(`the end of ${todo.length} process(es) of ${this.what}`, () => {
        const t = processTable();
        return todo.every((p) => !t.some((r) => same(r, p)));
      }, { noProgressMs: GONE_WITHIN_MS });
    } catch (e) {
      throw new Error(`reaper (${this.what}): pids ${todo.map((p) => p.pid).join(', ')} still ran after they were ended: ${e.message}`);
    }
    return todo.map(({ pid, how }) => ({ pid, how }));
  }
}

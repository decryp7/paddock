// The Node test kit (ADR 0103), for scripts/*.test.mjs and conformance/test. Not a test file: `node --test` runs only *.test.mjs.
//
// - nextPause and waitProgressing(what, done, { noProgressMs, progress, now, sleep }): waits until `done()` is true, polling with a
//   backoff (1 ms doubling to 100 ms, the replaced polls' own pace as the worst case), and fails only when `progress()` (a level:
//   lines written, files made; any change counts) has not changed for `noProgressMs`. Without `progress` that bound is a plain hang
//   guard from the start. `now` and `sleep` are injectable, so the schedule is tested without a wall clock.
// - Reaper: every process a test started ends with the test, and nothing else does (ER-14, rule 53). The same rules as
//   paddock_testkit_start::Reaper: it acts only on its own children (each in a session and process group of its own, with the
//   reaper's token PADDOCK_REAPER_ID in its environment), the descendants a sampler saw below them, and processes of this user
//   started after the reaper that are in one of its groups, carry its token (readable on Linux only: macOS shows no other
//   process's environment) or name a folder it made (scratchIn) while being orphans or children of this process. Each is ended alone, by pid after its start
//   time is checked again; never a process group, never by a pattern. With `{ inherit: true }` the token goes into this process's
//   own environment, so every child the test file starts by any means carries it.
// - processTable(): the table from `ps`; it throws when `ps` fails (a reaper that cannot look never says "nothing left"), and on
//   Windows, where the kit does not run (its tests skip with the reason).
import { spawn, spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { mkdirSync, readFileSync, realpathSync } from 'node:fs';
import { join } from 'node:path';
import { performance } from 'node:perf_hooks';

/** The first and the longest pause of a poll, in ms. */
export const FIRST_MS = 1;
export const LAST_MS = 100;
/** The first and the longest pause of a Reaper's sampler: one `ps` of the whole table blocks the event loop about 29 ms (863
 * processes, load 16), so it backs off to 0.5 s (about 6 % of the loop); what it misses after that the groups and the folder find. */
export const SAMPLE_FIRST_MS = 10;
export const SAMPLE_LAST_MS = 500;
/** How long reap() waits for what it ended to be gone: a bound that only stops a process the OS does not end. */
export const GONE_WITHIN_MS = 15_000;
/** The default no-progress bound: a hang guard under node --test's and CI's own bounds. */
export const HANG_GUARD_MS = 40_000;
/** The environment variable every child of a reaper carries. */
export const TOKEN_VAR = 'PADDOCK_REAPER_ID';

/** The pause after `previous` (0 for the first): doubles from FIRST_MS up to LAST_MS. */
export const nextPause = (previous) => (previous <= 0 ? FIRST_MS : Math.min(previous * 2, LAST_MS));

const realSleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Whether two progress levels are the same: numbers and strings by value (NaN equals NaN), arrays and objects by their JSON. */
export function sameLevel(a, b) {
  if (Object.is(a, b)) return true;
  if (a === null || b === null || typeof a !== 'object' || typeof b !== 'object') return false;
  return JSON.stringify(a) === JSON.stringify(b);
}

/**
 * Waits until `done()` (sync or async) is truthy and returns its value. Fails, naming `what`, the level and the time, only when
 * `progress()` has stood still for `noProgressMs` (default HANG_GUARD_MS); without `progress` that bound counts from the start.
 */
export async function waitProgressing(what, done, { noProgressMs = HANG_GUARD_MS, progress = () => 0, now = () => performance.now(), sleep = realSleep } = {}) {
  const began = now();
  let level = await progress();
  let since = began;
  let p = 0;
  for (;;) {
    const v = await done();
    if (v) return v;
    const at = await progress();
    if (!sameLevel(at, level)) { level = at; since = now(); }
    if (now() - since >= noProgressMs) {
      throw new Error(`${what} made no progress for ${noProgressMs} ms: it stood at ${JSON.stringify(level)} (after ${Math.round(now() - began)} ms in all; a slow host that keeps moving passes, only a stall fails)`);
    }
    p = nextPause(p);
    await sleep(p);
  }
}

/**
 * The process table: [{ pid, ppid, pgid, uid, started (ms), lstart, command }] (`ps`), zombies left out. Throws when `ps` fails
 * or on Windows: a caller that cannot look must not act as if nothing ran.
 */
export function processTable() {
  if (process.platform === 'win32') throw new Error('processTable: the test kit\'s process table is Unix only (ps); on Windows a test ends its children through their job');
  const r = spawnSync('ps', ['-A', '-ww', '-o', 'pid=,ppid=,pgid=,uid=,stat=,lstart=,command='], { encoding: 'utf8', env: { ...process.env, LC_ALL: 'C' } });
  if (r.error || r.status !== 0) throw new Error(`processTable: ps failed (${r.error?.message ?? `exit ${r.status}`}): ${String(r.stderr ?? '').trim().slice(0, 200)}`);
  const rows = [];
  for (const line of r.stdout.split('\n')) {
    // lstart is five words: "Sat Nov 14 21:41:09 2026".
    const m = /^\s*(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\S+)\s+(\S+\s+\S+\s+\S+\s+\S+\s+\S+)\s+(.*)$/.exec(line);
    if (!m || m[5].startsWith('Z')) continue;
    rows.push({ pid: Number(m[1]), ppid: Number(m[2]), pgid: Number(m[3]), uid: Number(m[4]), started: Date.parse(m[6]), lstart: m[6], command: m[7] });
  }
  if (rows.length === 0) throw new Error('processTable: ps listed no process');
  return rows;
}

/** Whether the environment of `pid` carries `NAME=value` (Linux, /proc; false where it cannot be read, as on macOS). */
export function carries(pid, name, value) {
  try {
    return readFileSync(`/proc/${pid}/environ`, 'latin1').split('\0').includes(`${name}=${value}`);
  } catch {
    return false;
  }
}

const same = (a, b) => a.pid === b.pid && a.lstart === b.lstart;

/** The rows of `table` that are the test's, with the first reason that holds (the pure rule; the same as the Rust reaper's). */
export function choose(table, { roots, below, groups, created, uid, me, token, folders, carry = carries }) {
  const out = [];
  // lstart has whole seconds, and Linux counts a start from a boot time in whole seconds (a process started 0.7 s after the
  // reaper showed 0.3 s before it in node:24-bookworm, 2026-11-15): one second of slack below the floor. The folder and the token
  // are made after the reaper and are random, so the slack can only admit a process of this user in one of its groups.
  const since = Math.floor(created / 1000) * 1000 - 1000;
  for (const row of table) {
    if (row.pid === me) continue;
    let how = null;
    if (roots.some((r) => same(r, row))) how = 'child';
    else if (below.some((b) => same(b, row))) how = 'below a child';
    else if (row.uid !== uid || !(row.started >= since)) how = null;
    else if (groups.has(row.pgid)) how = 'in a group the reaper made';
    else if (carry(row.pid, TOKEN_VAR, token)) how = "carries the reaper's token";
    else if ((row.ppid === 1 || row.ppid === me) && folders.some((f) => row.command.includes(f))) how = 'names a folder the reaper made';
    if (how) out.push({ pid: row.pid, lstart: row.lstart, how });
  }
  return out;
}

/** The guard for the processes of one test or test file (see the file's header). */
export class Reaper {
  /** @param {string} what a name for its messages; @param {{ inherit?: boolean }} options */
  constructor(what, { inherit = false } = {}) {
    if (process.platform === 'win32') throw new Error(`reaper (${what}): Unix only; on Windows a test ends its children through their job`);
    this.what = what;
    this.token = randomUUID();
    this.created = Date.now();
    this.roots = [];
    this.below = [];
    this.groups = new Set();
    this.folders = [];
    this.stopped = false;
    this.pace = 0;
    this.timer = null;
    if (inherit) process.env[TOKEN_VAR] = this.token;
    this.tick();
  }

  tick() {
    if (this.stopped) return;
    try { this.observe(); } catch { /* the reap looks again and fails there if it still cannot */ }
    this.pace = this.pace <= 0 ? SAMPLE_FIRST_MS : Math.min(this.pace * 2, SAMPLE_LAST_MS);
    this.timer = setTimeout(() => this.tick(), this.pace);
    this.timer.unref();
  }

  /** Stops the sampler (reap() does this itself). */
  stop() {
    this.stopped = true;
    clearTimeout(this.timer);
  }

  /** Starts a child (node's spawn) in a session and group of its own, with the token in its environment, and keeps its identity. */
  spawn(command, args = [], options = {}) {
    const env = { ...(options.env ?? process.env), [TOKEN_VAR]: this.token };
    const child = spawn(command, args, { ...options, env, detached: true });
    if (child.pid) {
      const row = processTable().find((p) => p.pid === child.pid);
      if (row) {
        this.roots.push({ pid: row.pid, lstart: row.lstart });
        if (row.pgid === row.pid) this.groups.add(row.pid);
      }
    }
    return child;
  }

  /** Makes a folder for the test under `parent`, named with the token, and returns it (an orphan that names it is the test's). */
  scratchIn(parent) {
    const dir = join(parent, `reaper-${this.token}-${this.folders.length}`);
    mkdirSync(dir);
    this.folders.push(dir);
    // macOS spells a temp folder two ways (/var and /private/var): a process may name either.
    try { const real = realpathSync(dir); if (real !== dir) this.folders.push(real); } catch { /* made above */ }
    return dir;
  }

  /** Looks at the process table now and remembers every process below a running child, and the groups they lead. */
  observe(table = processTable()) {
    const live = this.roots.filter((r) => table.some((p) => same(p, r)));
    const seen = new Set(live.map((r) => r.pid));
    let grew = true;
    while (grew) {
      grew = false;
      for (const p of table) {
        if (!seen.has(p.pid) && seen.has(p.ppid)) {
          seen.add(p.pid);
          if (!this.below.some((b) => same(b, p))) this.below.push({ pid: p.pid, lstart: p.lstart });
          if (p.pgid === p.pid) this.groups.add(p.pid);
          grew = true;
        }
      }
    }
  }

  /** The processes reap() would end now, and why. */
  candidates(table = processTable()) {
    return choose(table, { roots: this.roots, below: this.below, groups: this.groups, created: this.created, uid: process.getuid(), me: process.pid, token: this.token, folders: this.folders });
  }

  /** Ends everything still running that the test created, each alone, waits until it is gone and returns what it had to end. */
  async reap() {
    this.stop();
    const table = processTable();
    this.observe(table);
    const todo = this.candidates(table);
    for (const p of todo) {
      // Only while the pid is still that process (its start time), and only that process: never its group.
      if (!processTable().some((t) => same(t, p))) continue;
      try { process.kill(p.pid, 'SIGKILL'); } catch { /* gone */ }
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

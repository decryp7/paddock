// C30: Rust tests do not assert on the wall clock (rule 40; lesson "Tests that measure the wall clock fail under load").
// Inside a `#[test]` fn, an `assert!`/`assert_eq!`/`assert_ne!` that compares (`<`, `<=`, `>`, `>=`) a duration measured with
// `elapsed()`, `Instant::now() - x` or `duration_since` (or a variable assigned from one) is flagged: under load the limit
// fails. Use a `FakeClock` (paddock_jobs::time, re-exported by paddock-testkit) or count items instead. A hang guard
// (a generous limit that only stops a hung test) is allowed with `// conformance: hang-guard` on the assertion's line
// or the line above and a limit of at least 10 s. Also flagged: a `thread::sleep` in a Rust test that also starts or joins
// threads, uses channels, atomics or locks (a sleep waiting for another thread's effect: wait on the event, a channel or the
// FakeClock instead; the waiver comment alone allows a deliberate sleep), a non-negated `assert!` over `.close()` / `.close_gracefully(..)`
// (it asserts that a product grace period was long enough for this machine; waive `close_gracefully` with the hang-guard comment
// and a grace of at least 10 s, or either with `// conformance: grace-free — <why>` when the thing ended already), and, in TypeScript specs, `Date.now()` /
// `performance.now()` differences asserted with `expect` (unless the spec uses fake timers: `useFakeTimers`, `setSystemTime`,
// `page.clock`). Warning level: existing code is listed, never blocks a build; except an assertion over a grace period, an
// error since it failed CI again as a warning (ER-21).
import { error, warn } from '../finding.mjs';

const HANG_GUARD = /conformance:\s*hang-guard/;
/** The waiver of a sleep must say why: `// conformance: hang-guard — <reason>` (a dash, an en dash or a hyphen, then words). */
const SLEEP_WAIVER = /conformance:\s*hang-guard\s*[—–-]+\s*(\S.{2,})/;
const MIN_GUARD_MS = 10_000;
/** CI's budget for one test: nextest's ci profile reports it slow after 30 s and kills it after 2 periods (60 s). A hang
 * guard at or above it never reports: the test is killed first, without its diagnostics. */
const CI_TEST_BUDGET_MS = 60_000;
const OVER_BUDGET = (ms, text) => `a hang guard of ${ms / 1000} s is at or above CI's 60 s test budget (nextest kills the test first, without its message): ${text}`;
const OVER_BUDGET_FIX = 'Lower the guard below 60 s (40 s is over three times the slowest browser exit measured under load, 11.7 s, docs/benchmarks.md).';
// A measurement: a duration read off a real clock. A bare `Instant::now()` used as the base of a computed instant is deterministic.
const CLOCK_READ = /\.elapsed\s*\(\)|\.duration_since\s*\(|\b(?:Instant|SystemTime)::now\s*\(\)\s*-/;

/** `text` with comments and the insides of string/char literals blanked (newlines kept), so braces and parentheses in them do not count. */
export function mask(text) {
  let out = '';
  for (let i = 0; i < text.length; ) {
    const c = text[i];
    const two = text.slice(i, i + 2);
    if (two === '//') {
      const end = text.indexOf('\n', i);
      const stop = end < 0 ? text.length : end;
      out += ' '.repeat(stop - i);
      i = stop;
    } else if (two === '/*') {
      const end = text.indexOf('*/', i + 2);
      const stop = end < 0 ? text.length : end + 2;
      out += text.slice(i, stop).replace(/[^\n]/g, ' ');
      i = stop;
    } else if (c === '"') {
      let j = i + 1;
      while (j < text.length && text[j] !== '"') j += text[j] === '\\' ? 2 : 1;
      out += `"${text.slice(i + 1, j).replace(/[^\n]/g, ' ')}"`;
      i = j + 1;
    } else {
      out += c;
      i++;
    }
  }
  return out;
}

/** Index of the bracket closing the one at `open` (in masked text), or -1. */
function closing(text, open) {
  const [o, c] = text[open] === '{' ? ['{', '}'] : ['(', ')'];
  let depth = 0;
  for (let i = open; i < text.length; i++) {
    if (text[i] === o) depth++;
    else if (text[i] === c && --depth === 0) return i;
  }
  return -1;
}

/** The smallest hang-guard limit written as `Duration::from_secs(N)` / `from_millis(N)` in `args`, in ms (null when none is written). */
function literalLimitMs(args) {
  const limits = [...args.matchAll(/from_(secs|millis)\s*\(\s*([\d_]+)\s*\)/g)].map((m) => Number(m[2].replace(/_/g, '')) * (m[1] === 'secs' ? 1000 : 1));
  return limits.length ? Math.min(...limits) : null;
}

/** Wall-clock assertions in one Rust file: [{ line, text, guard }] where `guard` is null, or the limit in ms of a waived hang guard. */
export function wallClockAsserts(source) {
  const masked = mask(source);
  const lineOf = (i) => masked.slice(0, i).split('\n').length;
  const out = [];
  for (const t of masked.matchAll(/#\[(?:[\w]+::)*test(?:\([^)]*\))?\]/g)) {
    const fn = /\bfn\s+\w+[^{;]*\{/.exec(masked.slice(t.index));
    if (!fn) continue;
    const open = t.index + fn.index + fn[0].length - 1;
    const end = closing(masked, open);
    if (end < 0) continue;
    const body = masked.slice(open, end);
    const tainted = new Set();
    for (const m of body.matchAll(/\blet\s+(?:mut\s+)?(\w+)\s*(?::[^=;]+)?=([^;]*);/g)) {
      if (CLOCK_READ.test(m[2]) || [...tainted].some((v) => new RegExp(`\\b${v}\\b`).test(m[2]))) tainted.add(m[1]);
    }
    for (const a of body.matchAll(/\b(?:debug_)?assert(?:_eq|_ne)?!\s*\(/g)) {
      const start = open + a.index + a[0].length - 1;
      const stop = closing(masked, start);
      if (stop < 0) continue;
      const args = masked.slice(start, stop);
      const reads = CLOCK_READ.test(args) || [...tainted].some((v) => new RegExp(`\\b${v}\\b`).test(args));
      const limited = /[<>]=?/.test(args.replace(/->|=>|::<[^>]*>/g, ''));
      if (!reads || !limited) continue;
      const line = lineOf(start);
      const raw = source.split('\n');
      const guarded = HANG_GUARD.test(raw[line - 1] ?? '') || HANG_GUARD.test(raw[line - 2] ?? '');
      out.push({ line, text: (raw[line - 1] ?? '').trim().slice(0, 100), guard: guarded ? (literalLimitMs(args) ?? MIN_GUARD_MS) : null });
    }
  }
  return out;
}

/** Calls that wait out a product's grace period and answer whether it was enough: `.close()` with no argument (a browser's own) and `.close_gracefully(..)`. `.close(id)` (a store, a renderer) is something else. */
const GRACE_CALL = /\.close\s*\(\s*\)|\.close_gracefully\s*\(/;
/** The waiver of an assert over a call that cannot wait: `// conformance: grace-free — <why>` (the thing has ended already, so no grace is waited out). */
const GRACE_FREE = /conformance:\s*grace-free\s*[—–-]+\s*(\S.{2,})/;

/**
 * Asserts over a call that waits out a grace period, in `#[test]` fns of a Rust file: [{ line, text, call, waiver, limitMs }].
 * `waiver` is 'grace-free' (with a reason), 'hang-guard' (only for `close_gracefully`, whose limit `limitMs` is the literal
 * `Duration` written, null when none) or null. Negated asserts (`assert!(!b.close_gracefully(Duration::ZERO))`: forced end,
 * nothing waited for) are not reported.
 */
export function graceAsserts(source) {
  const masked = mask(source);
  const raw = source.split('\n');
  const out = [];
  for (const t of masked.matchAll(/#\[(?:[\w]+::)*test(?:\([^)]*\))?\]/g)) {
    const fn = /\bfn\s+\w+[^{;]*\{/.exec(masked.slice(t.index));
    if (!fn) continue;
    const open = t.index + fn.index + fn[0].length - 1;
    const end = closing(masked, open);
    if (end < 0) continue;
    const body = masked.slice(open, end);
    for (const a of body.matchAll(/\b(?:debug_)?assert(?:_eq|_ne)?!\s*\(/g)) {
      const start = open + a.index + a[0].length - 1;
      const stop = closing(masked, start);
      if (stop < 0) continue;
      const args = masked.slice(start, stop);
      const call = GRACE_CALL.exec(args);
      if (!call || /^\(\s*!/.test(args)) continue;
      const line = masked.slice(0, start).split('\n').length;
      const near = [raw[line - 1] ?? '', raw[line - 2] ?? ''];
      const gracious = call[0].includes('close_gracefully');
      const waiver = near.some((l) => GRACE_FREE.test(l)) ? 'grace-free' : gracious && near.some((l) => HANG_GUARD.test(l)) ? 'hang-guard' : null;
      out.push({ line, text: (raw[line - 1] ?? '').trim().slice(0, 100), call: call[0], waiver, limitMs: gracious ? literalLimitMs(args) : null });
    }
  }
  return out;
}

/** Signals in a test body that a sleep is waiting for another thread. */
const THREADED = /\bthread::(?:spawn|scope)\b|\.spawn\s*\(|\.join\s*\(\)|\bmpsc\b|\bchannel\s*\(|\.recv(?:_timeout)?\s*\(|\bAtomic\w+|\bArc::|\bMutex\b|\bCondvar\b/;

/** `thread::sleep` calls in threaded `#[test]` fns of a Rust file: [{ line, text, waived }] (`waived`: false, 'reason', or 'no-reason' when the comment gives none). */
export function threadSleeps(source) {
  const masked = mask(source);
  const raw = source.split('\n');
  const out = [];
  for (const t of masked.matchAll(/#\[(?:[\w]+::)*test(?:\([^)]*\))?\]/g)) {
    const fn = /\bfn\s+\w+[^{;]*\{/.exec(masked.slice(t.index));
    if (!fn) continue;
    const open = t.index + fn.index + fn[0].length - 1;
    const end = closing(masked, open);
    if (end < 0) continue;
    const body = masked.slice(open, end);
    if (!THREADED.test(body)) continue;
    for (const m of body.matchAll(/\b(?:thread::)?sleep\s*\(/g)) {
      if (!/thread::sleep|std::thread/.test(m[0]) && !/\buse\s+std::thread::sleep\b/.test(masked)) continue;
      const line = masked.slice(0, open + m.index).split('\n').length;
      const near = [raw[line - 1] ?? '', raw[line - 2] ?? ''];
      const waived = near.some((l) => SLEEP_WAIVER.test(l)) ? 'reason' : near.some((l) => HANG_GUARD.test(l)) ? 'no-reason' : false;
      out.push({ line, text: (raw[line - 1] ?? '').trim().slice(0, 100), waived });
    }
  }
  return out;
}

const FAKE_TIMERS = /\buseFakeTimers\b|\bsetSystemTime\b|\bpage\.clock\b|\bjasmine\.clock\b/;
const NOW = /\b(?:Date|performance)\.now\s*\(\)/;

/** Wall-clock differences asserted in a TypeScript spec: [{ line, text, guard }] (guard: null, or the waived limit in ms). */
export function tsWallClockAsserts(source) {
  if (FAKE_TIMERS.test(source)) return [];
  const clean = mask(source);
  const raw = source.split('\n');
  const readings = new Set();
  for (const m of clean.matchAll(/\b(?:const|let|var)\s+(\w+)\s*=\s*(?:Date|performance)\.now\s*\(\)\s*;/g)) readings.add(m[1]);
  const diff = (text) => {
    const names = [...readings].map((n) => `\\b${n}\\b`);
    const operand = `(?:${[NOW.source, ...names, '[\\w.]+'].join('|')})`;
    return new RegExp(`(?:${NOW.source}|${names.join('|') || '(?!)'})\\s*-\\s*${operand}|${operand}\\s*-\\s*(?:${NOW.source}${names.length ? `|${names.join('|')}` : ''})`).test(text);
  };
  const measured = new Set();
  for (const m of clean.matchAll(/\b(?:const|let|var)\s+(\w+)\s*=\s*([^;]*);/g)) if (diff(m[2])) measured.add(m[1]);
  const out = [];
  for (const a of clean.matchAll(/\bexpect\s*\(/g)) {
    const start = a.index + a[0].length - 1;
    const stop = closing(clean, start);
    if (stop < 0) continue;
    const arg = clean.slice(start, stop);
    const rest = clean.slice(stop, stop + 160).split(';')[0];
    const reads = diff(arg) || [...measured].some((v) => new RegExp(`\\b${v}\\b`).test(arg));
    if (!reads || !/\.(?:not\.)?toBe(?:Less|Greater)Than(?:OrEqual)?\s*\(|\.toBeCloseTo\s*\(/.test(rest)) continue;
    const line = clean.slice(0, start).split('\n').length;
    const guarded = HANG_GUARD.test(raw[line - 1] ?? '') || HANG_GUARD.test(raw[line - 2] ?? '');
    const limit = /toBe(?:Less|Greater)Than(?:OrEqual)?\s*\(\s*([\d_]+)/.exec(rest);
    out.push({ line, text: (raw[line - 1] ?? '').trim().slice(0, 100), guard: guarded ? (limit ? Number(limit[1].replace(/_/g, '')) : MIN_GUARD_MS) : null });
  }
  return out;
}

export default {
  id: 'C30',
  title: 'Rust tests do not assert on the wall clock',
  rule: 40,
  run(ctx) {
    const out = [];
    const fix = 'Use a FakeClock (paddock-testkit re-exports paddock_jobs::time::FakeClock) or count items; a hang guard of at least 10 s may carry `// conformance: hang-guard`.';
    for (const f of ctx.files()) {
      if (f.endsWith('.rs')) {
        const text = ctx.read(f);
        if (!text || !/#\[(?:\w+::)*test/.test(text)) continue;
        for (const a of wallClockAsserts(text)) {
          if (a.guard === null) out.push(warn('C30', 40, f, a.line, `asserts on the wall clock: ${a.text}`, fix));
          else if (a.guard < MIN_GUARD_MS) out.push(warn('C30', 40, f, a.line, `a hang guard of ${a.guard} ms is too tight to be one: ${a.text}`, 'Raise the limit to at least 10 s, or replace it with a FakeClock.'));
          else if (a.guard >= CI_TEST_BUDGET_MS) out.push(warn('C30', 40, f, a.line, OVER_BUDGET(a.guard, a.text), OVER_BUDGET_FIX));
        }
        for (const a of graceAsserts(text)) {
          if (a.waiver === 'grace-free') continue;
          if (a.waiver === 'hang-guard') {
            if (a.limitMs !== null && a.limitMs < MIN_GUARD_MS) out.push(warn('C30', 40, f, a.line, `a hang guard of ${a.limitMs} ms is too tight to be one: ${a.text}`, 'Raise the grace to at least 10 s (the exit is waited for on the condition, so a long guard costs nothing), or assert the mechanism instead.'));
            else if (a.limitMs !== null && a.limitMs >= CI_TEST_BUDGET_MS) out.push(warn('C30', 40, f, a.line, OVER_BUDGET(a.limitMs, a.text), OVER_BUDGET_FIX));
            continue;
          }
          // An error, not a warning (ER-21): this kind failed CI again while it was only a warning (fix/flaky-browser-close).
          out.push(error('C30', 40, f, a.line, `asserts that a call outlasted by load finished within its grace: ${a.text}`, 'Wait for the exit on the condition with a hang guard (`close_gracefully(Duration::from_secs(40))` with `// conformance: hang-guard`: over three times the slowest exit measured under load, 11.7 s, and under CI\'s 60 s test kill), assert the mechanism, not the budget, or, when the thing has ended already, say so with `// conformance: grace-free — <why>` (lesson "Chromium tests compared a loaded machine with a product timeout").'));
        }
        for (const s of threadSleeps(text)) {
          if (s.waived === 'no-reason') out.push(warn('C30', 40, f, s.line, `the sleep's waiver gives no reason: ${s.text}`, 'Write `// conformance: hang-guard — <why this sleep is needed>`.'));
          else if (!s.waived) out.push(warn('C30', 40, f, s.line, `sleeps in a test that uses threads: ${s.text}`, 'Wait for the event (a channel, a gate, `FakeClock::wait_for_waiters`) instead of a sleep; a deliberate sleep carries `// conformance: hang-guard — <why>`.'));
        }
      } else if (/\.(?:spec|test|e2e|shot)\.[mc]?[jt]sx?$|(?:^|\/)(?:e2e|tests?)\/[^/]*\.[mc]?[jt]sx?$/.test(f)) {
        const text = ctx.read(f);
        if (!text || !NOW.test(text)) continue;
        for (const a of tsWallClockAsserts(text)) {
          if (a.guard === null) out.push(warn('C30', 40, f, a.line, `asserts on the wall clock: ${a.text}`, 'Use fake timers (vi.useFakeTimers, page.clock) or assert on events; a hang guard of at least 10 s may carry `// conformance: hang-guard`.'));
          else if (a.guard < MIN_GUARD_MS) out.push(warn('C30', 40, f, a.line, `a hang guard of ${a.guard} ms is too tight to be one: ${a.text}`, 'Raise the limit to at least 10 s, or use fake timers.'));
        }
      }
    }
    return out;
  },
};

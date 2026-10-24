// C30: Rust tests do not assert on the wall clock (rule 40; lesson "Tests that measure the wall clock fail under load").
// Inside a `#[test]` fn, an `assert!`/`assert_eq!`/`assert_ne!` that compares (`<`, `<=`, `>`, `>=`) a duration measured with
// `elapsed()`, `Instant::now() - x` or `duration_since` (or a variable assigned from one) is flagged: under load the limit
// fails. Use a `FakeClock` (paddock_jobs::time, re-exported by paddock-testkit) or count items instead. A hang guard
// (a generous limit that only stops a hung test) is allowed with `// conformance: hang-guard` on the assertion's line
// or the line above and a limit of at least 10 s. Warning level: existing code is listed, never blocks a build.
import { warn } from '../finding.mjs';

const HANG_GUARD = /conformance:\s*hang-guard/;
const MIN_GUARD_MS = 10_000;
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

export default {
  id: 'C30',
  title: 'Rust tests do not assert on the wall clock',
  rule: 40,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs')) continue;
      const text = ctx.read(f);
      if (!text || !/#\[(?:\w+::)*test/.test(text)) continue;
      for (const a of wallClockAsserts(text)) {
        if (a.guard === null) {
          out.push(warn('C30', 40, f, a.line, `asserts on the wall clock: ${a.text}`, 'Use a FakeClock (paddock-testkit re-exports paddock_jobs::time::FakeClock) or count items; a hang guard of at least 10 s may carry `// conformance: hang-guard`.'));
        } else if (a.guard < MIN_GUARD_MS) {
          out.push(warn('C30', 40, f, a.line, `a hang guard of ${a.guard} ms is too tight to be one: ${a.text}`, 'Raise the limit to at least 10 s, or replace it with a FakeClock.'));
        }
      }
    }
    return out;
  },
};

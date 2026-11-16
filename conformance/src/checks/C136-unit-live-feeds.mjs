// C136: a unit test that starts a live mock feed runs it on a clock the test moves (ER-21 class "a test asserts a value while a live
// mock feed keeps changing it"; instance: Debrief's events-grid live tail, run 8191; related, in e2e, the Dyno row menu under live
// updates, which the e2e kit's `pauseLive` / `withLivePaused` cover). Common practice for a feed on timers is a clock the test
// advances (Debrief's `ManualClock` in apps/debrief/app/src/testing/manual-clock.ts, which every `MockBackend` takes as `clock`) or
// Vitest's fake timers, never the page's real ones.
// Flagged: a `MockBackend(` or `synthetic(` call in a unit spec (`*.spec.ts` under `apps/` or `ui/`, not an e2e folder) whose options (any
// number of lines) have `liveMs: <literal>` below one second, a feed that ticks on the page's own timers (a long `liveMs` never ticks and
// is not flagged; only literals are judged, a `liveMs` held in a constant is not read). Each call is judged by itself: it is covered when
// its own options have `clock:` or the shorthand `clock`, or when its `it(` / `test(` block, or a `beforeEach` that encloses it, calls
// `useFakeTimers`. A mention of `ManualClock` elsewhere in the file (an import) covers nothing. Findings are warnings: report-only until
// its first green run on the real tree is recorded here (ER-41). Recorded: not yet.
import { warn } from '../finding.mjs';

const SPEC = /^(?:apps|ui)\/.*\.spec\.ts$/;
const SKIP = /(?:^|\/)(?:node_modules|dist|e2e)\//;
const FAKES = /\buseFakeTimers\s*\(/;
const CLOCK_OPTION = /\bclock\b\s*[:,}]/;
const BEFORE_EACH = /^(\s*)beforeEach\s*\(/;
const BLOCK = /^\s*(?:it|test)(?:\.\w+)*\s*\(/;
const CALL = /(?:\bMockBackend\b\s*\(|\bsynthetic\s*\()/g;
const FIX = 'Run the feed on a clock the test moves: `new MockBackend({ liveMs, clock })` with a `ManualClock` and `await clock.advance(ms)` per tick (apps/debrief/app/src/testing/manual-clock.ts), or `vi.useFakeTimers()`.';

const lineOf = (text, index) => text.slice(0, index).split('\n').length;

/** The text of the call whose opening parenthesis ends at `from` (up to its closing parenthesis). */
function span(text, from) {
  let depth = 1;
  let i = from;
  for (; i < text.length && depth > 0; i++) {
    if (text[i] === '(') depth++;
    else if (text[i] === ')') depth--;
  }
  return text.slice(from, i);
}

/** The 0-based line ranges [from, to) in which a `beforeEach` that calls `useFakeTimers` is in force (its end to the end of its scope). */
function fakedScopes(lines) {
  const scopes = [];
  lines.forEach((l, i) => {
    const m = BEFORE_EACH.exec(l);
    if (!m) return;
    const indent = m[1].length;
    const closes = (j) => /^\s*\}/.test(lines[j]) && lines[j].length - lines[j].trimStart().length <= indent;
    let end = i;
    while (end < lines.length - 1 && !(end > i && closes(end)) && !(end === i && /\}\)\s*;?\s*$/.test(l))) end++;
    if (!FAKES.test(lines.slice(i, end + 1).join('\n'))) return;
    let scope = end + 1;
    while (scope < lines.length && !(closes(scope) && lines[scope].length - lines[scope].trimStart().length < indent)) scope++;
    scopes.push([end + 1, scope]);
  });
  return scopes;
}

/** [{ line, why }] of every ticking live-feed start in `text` that no clock the test moves covers (each call judged by itself). */
export function liveStarts(text) {
  const lines = text.split('\n');
  const starts = [];
  lines.forEach((l, i) => {
    if (BLOCK.test(l)) starts.push(i);
  });
  // Blocks run from one it( to the next (the last to the end).
  const blocks = starts.map((s, k) => ({ from: s, to: k + 1 < starts.length ? starts[k + 1] : lines.length }));
  const scopes = fakedScopes(lines);
  const found = [];
  for (const m of text.matchAll(CALL)) {
    const callLine = lineOf(text, m.index);
    if (lineOfComment(lines, callLine)) continue;
    const opts = span(text, m.index + m[0].length);
    const live = /\bliveMs\s*:\s*(\d{1,3})\b(?![_\d])/.exec(opts);
    if (!live || CLOCK_OPTION.test(opts)) continue;
    const i = callLine - 1;
    const block = blocks.find((b) => i >= b.from && i < b.to);
    if (block && FAKES.test(lines.slice(block.from, block.to).join('\n'))) continue;
    if (scopes.some(([from, to]) => i >= from && i < to)) continue;
    found.push({ line: lineOf(text, m.index + m[0].length + live.index), why: `starts a live mock feed on the page's timers (\`liveMs: ${live[1]}\`)` });
  }
  return found.sort((a, b) => a.line - b.line);
}

const lineOfComment = (lines, n) => /^\s*(?:\/\/|\*)/.test(lines[n - 1] ?? '');

export default {
  id: 'C136',
  scope: 'root',
  mode: 'report-only',
  title: 'A unit test that starts a live mock feed runs it on a clock the test moves',
  rule: 21,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!SPEC.test(f) || SKIP.test(f)) continue;
      const text = ctx.read(f);
      if (text === null || text === undefined) {
        out.push(warn('C136', 21, f, 1, 'not measured: the file cannot be read', 'Fix the file, then run the check again.'));
        continue;
      }
      for (const { line, why } of liveStarts(text)) out.push(warn('C136', 21, f, line, `${why} and its call has no clock option and no useFakeTimers covers it`, FIX));
    }
    return out;
  },
};

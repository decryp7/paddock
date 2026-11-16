// C136: a unit test that starts a live mock feed runs it on a clock the test moves (ER-21 class "a test asserts a value while a live
// mock feed keeps changing it"; instance: Debrief's events-grid live tail, run 8191; related, in e2e, the Dyno row menu under live
// updates, which the e2e kit's `pauseLive` / `withLivePaused` cover). Common practice for a feed on timers is a clock the test
// advances (Debrief's `ManualClock` in apps/debrief/app/src/testing/manual-clock.ts, which every `MockBackend` takes as `clock`) or
// Vitest's fake timers, never the page's real ones.
// Flagged: an `it(` / `test(` block of a unit spec (`*.spec.ts` under `apps/` or `ui/`, not an e2e folder) that starts a live feed on
// the page's own timers: a `MockBackend(` or `synthetic(` call whose options (any number of lines) have `liveMs: <n>` below one second
// (a long `liveMs` never ticks and is not flagged); unless the same block, or the code outside every block of
// the file (a shared `setup`), uses `ManualClock`, `useFakeTimers` or a `clock:` option. Findings are warnings: report-only until its
// first green run on the real tree is recorded here (ER-41). Recorded: not yet.
import { warn } from '../finding.mjs';

const SPEC = /^(?:apps|ui)\/.*\.spec\.ts$/;
const SKIP = /(?:^|\/)(?:node_modules|dist|e2e)\//;
const MOVES = /\b(?:ManualClock|useFakeTimers)\b|\bclock\s*:/;
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

/** [{ line, why }] of every live-feed start in `text` that no clock the test moves covers (judged per it() block). */
export function liveStarts(text) {
  const lines = text.split('\n');
  const starts = [];
  lines.forEach((l, i) => {
    if (BLOCK.test(l)) starts.push(i);
  });
  // Blocks run from one it( to the next (the last to the end); the rest of the file is shared code.
  const blocks = starts.map((s, k) => ({ from: s, to: k + 1 < starts.length ? starts[k + 1] : lines.length }));
  const inBlock = (i) => blocks.find((b) => i >= b.from && i < b.to);
  const sharedMoves = MOVES.test(lines.filter((_, i) => !inBlock(i)).join('\n'));
  const found = [];
  const note = (index, why) => {
    const line = lineOf(text, index);
    const block = inBlock(line - 1);
    if (sharedMoves || (block && MOVES.test(lines.slice(block.from, block.to).join('\n')))) return;
    found.push({ line, why });
  };
  for (const m of text.matchAll(CALL)) {
    if (lineOfComment(lines, lineOf(text, m.index))) continue;
    const opts = span(text, m.index + m[0].length);
    const live = /\bliveMs\s*:\s*(\d{1,3})\b(?![_\d])/.exec(opts);
    if (live) note(m.index + m[0].length + live.index, `starts a live mock feed on the page's timers (\`liveMs: ${live[1]}\`)`);
  }
  return found.sort((a, b) => a.line - b.line);
}

const lineOfComment = (lines, n) => /^\s*(?:\/\/|\*)/.test(lines[n - 1] ?? '');

export default {
  id: 'C136',
  scope: 'root',
  mode: 'report-only',
  title: 'A unit test that starts a live mock feed holds it before it asserts',
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
      for (const { line, why } of liveStarts(text)) out.push(warn('C136', 21, f, line, `${why} and neither its block nor the shared code of the file moves a clock or fakes the timers`, FIX));
    }
    return out;
  },
};

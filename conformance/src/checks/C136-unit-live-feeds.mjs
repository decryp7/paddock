// C136: a unit test that starts a live mock feed holds it before it asserts (ER-21 class "a test asserts a value while a live mock
// feed or clock keeps changing it": Debrief's events-grid live tail, run 8191; the Spotter Today clock, run 8154; the Dyno row menu,
// run 8161; Debrief's start-up settle, run 8170). The unit kit has `holdableTimers`, `holdLive`, `releaseLive` and `withLiveHeld`
// (`@decryp7/paddock-ui/testing`, ui/projects/paddock-ui/testing/src/live.ts); the e2e kit has `pauseLive` and `withLivePaused`.
// Flagged: a line of a unit spec (`*.spec.ts` under `apps/` or `ui/`, not an e2e folder) that starts a live feed on the page's own
// timers: a mock built with `liveMs: <n>` below one second (a feed that ticks while the test runs; a long `liveMs` never ticks and is not flagged) or a call of `onLive(`; unless the file also
// uses the kit's hold (`holdableTimers`, `holdLive`, `withLiveHeld`), fake timers (`useFakeTimers`) or a clock the test moves
// (`ManualClock`). Heuristic, by file: a file that holds in one test is not judged per test. Findings are warnings: report-only until
// its first green run on the real tree is recorded here (ER-41). Recorded: not yet.
import { warn } from '../finding.mjs';

const SPEC = /^(?:apps|ui)\/.*\.spec\.ts$/;
const SKIP = /(?:^|\/)(?:node_modules|dist|e2e)\//;
const HOLDS = /\b(?:holdableTimers|holdLive|withLiveHeld|useFakeTimers|ManualClock)\b/;
const STARTS = [
  { re: /(?:\bMockBackend\b|\bsynthetic\().*\bliveMs\s*:\s*(\d{1,3})\b(?![_\d])/, why: (m) => `starts a live mock feed on the page's timers (\`liveMs: ${m[1]}\`)` },
  { re: /\.onLive\s*\(/, why: () => 'subscribes to a live feed (`onLive(`)' },
];
const FIX = 'Run the feed on a clock the test moves and wrap what it asserts in `withLiveHeld(...)` (or `holdLive()` / `releaseLive()`), from `@decryp7/paddock-ui/testing`: `holdableTimers(new ManualClock())` for a mock that takes a clock; see ui/projects/paddock-ui/testing/src/live.ts.';

/** [{ line, why }] of every live-feed start in `text`, or none when the file holds, fakes the timers or moves its own clock. */
export function liveStarts(text) {
  if (HOLDS.test(text)) return [];
  const out = [];
  text.split('\n').forEach((l, i) => {
    if (/^\s*(?:\/\/|\*)/.test(l)) return;
    for (const { re, why } of STARTS) {
      const m = re.exec(l);
      if (m) {
        out.push({ line: i + 1, why: why(m) });
        break;
      }
    }
  });
  return out;
}

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
      for (const { line, why } of liveStarts(text)) out.push(warn('C136', 21, f, line, `${why} and the file never holds it, fakes the timers or moves a clock`, FIX));
    }
    return out;
  },
};

// C144 (ER-21 class "a whole streamed answer bounded by one expect timeout": debrief.yml runs 8300 and 8377, ai-chat.e2e.ts:116 waited for the
// stream's `aria-busy` to become "false" inside one 5 s expect while the answer was still arriving, in both tries).
// `expectStreamDone` (ui/projects/paddock-e2e/src/ready.ts, ADR 0103) waits on PROGRESS: the wait restarts while the stream still delivers and fails
// only when it stops. An e2e spec waits for a stream to finish only through it. Flagged, in a Playwright spec under `apps/<app>/app/e2e/`, `ui/e2e/`
// or the kit (`ui/projects/paddock-e2e/`), only the wait for the stream to FINISH: `toHaveAttribute('aria-busy', 'false')` and a locator, `waitForSelector`
// or `waitForFunction` selector over `[aria-busy=false]` / `[aria-busy="false"]`. Not flagged: `aria-busy` asserted "true" (a test that busy starts), counts
// of `[aria-busy="true"]` (loading skeletons), `getAttribute` reads and negated-true page-settle waits. Allowed: `ready.ts`, its unit test `ready.test.ts` and
// the helper's selftest (`selftest/stream-done.spec.ts`). A site that is not a stream wait carries `// conformance: stream-wait — <why>` on its line or the
// line above. Limit: other signals (`data-*` flags) are not read; no parser.
// ER-41: report-only (warnings) until its first green run on the real tree, recorded by run id here. Recorded: not yet.
import { warn } from '../finding.mjs';
import { maskTs } from './C71-test-timing.mjs';

export const SEVERITY = warn;
const SPEC = /^(?:apps\/[^/]+\/app\/e2e\/.+|ui\/e2e\/.+|ui\/projects\/paddock-e2e\/.+)\.ts$/;
const ALLOWED = new Set([
  'ui/projects/paddock-e2e/src/ready.ts',
  'ui/projects/paddock-e2e/src/ready.test.ts',
  'ui/projects/paddock-e2e/selftest/stream-done.spec.ts',
]);
const WAIVER = /conformance:\s*stream-wait\s*[—–-]+\s*\S.{2,}/;

// The two shapes of a wait for a stream to FINISH: the attribute asserted "false", and a selector or waitFor over `[aria-busy=false]`.
const FINISHED = [
  /toHaveAttribute\(\s*['"`]aria-busy['"`]\s*,\s*['"`]false['"`]/g,
  /aria-busy\s*=\s*(['"]?)false\1\s*\]/g,
];

/** [{ line }] of the waits for `aria-busy` to be "false" in code (not a comment), without a `stream-wait` waiver on the line or the line above. */
export function bareStreamWaits(text) {
  const lines = text.split('\n');
  const masked = maskTs(text).split('\n');
  const out = new Set();
  for (const re of FINISHED) {
    for (const m of text.matchAll(re)) {
      const at = m.index + m[0].indexOf('aria-busy');
      const line = text.slice(0, at).split('\n').length - 1;
      const col = at - (text.lastIndexOf('\n', at - 1) + 1);
      // Code, not a comment: maskTs blanks comments whole but leaves the quotes of a string, so a quote stands on the masked line on both sides.
      if (!/['"`]/.test(masked[line].slice(0, col)) || !/['"`]/.test(masked[line].slice(col + 9))) continue;
      if (WAIVER.test(lines[line]) || WAIVER.test(lines[line - 1] ?? '')) continue;
      out.add(line + 1);
    }
  }
  return [...out].sort((x, y) => x - y).map((line) => ({ line }));
}

export default {
  id: 'C144',
  scope: 'root',
  mode: 'report-only',
  title: 'An e2e spec waits for a stream to finish only through expectStreamDone',
  rule: 21,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!SPEC.test(f) || ALLOWED.has(f)) continue;
      const text = ctx.read(f);
      if (!text || !text.includes('aria-busy')) continue;
      for (const hit of bareStreamWaits(text)) {
        out.push(SEVERITY('C144', 21, f, hit.line, 'waits for `aria-busy` to be "false" itself: one expect timeout then bounds a whole streamed answer (runs 8300, 8377)', 'Call `expectStreamDone(stream)` from @decryp7/paddock-e2e (it waits on progress), or, if this is not a stream-finished wait, add `// conformance: stream-wait — <why>`.'));
      }
    }
    return out;
  },
};

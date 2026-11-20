// C144 (ER-21 class "a whole streamed answer bounded by one expect timeout": debrief.yml runs 8300 and 8377, ai-chat.e2e.ts:116 waited for the
// stream's `aria-busy` to become "false" inside one 5 s expect while the answer was still arriving, in both tries).
// `expectStreamDone` (ui/projects/paddock-e2e/src/ready.ts, ADR 0103) waits on PROGRESS: the wait restarts while the stream still delivers and fails
// only when it stops. An e2e spec waits for a stream to finish only through it. Flagged, in a Playwright spec under `apps/<app>/app/e2e/`, `ui/e2e/`
// or the kit (`ui/projects/paddock-e2e/`): any use of `aria-busy` outside comments (`toHaveAttribute('aria-busy', ..)`, `getAttribute`, a
// `[aria-busy=..]` selector, a `waitForFunction`/`waitForSelector` over it). Allowed: `ready.ts` itself, its unit test `ready.test.ts` and the
// selftest of the helper (`selftest/stream-done.spec.ts`). A site that is not a stream-finished wait (a loading skeleton that is counted or shown)
// carries `// conformance: stream-wait — <why>` on its line or the line above. Limit: other signals (`data-*` flags) are not read; no parser.
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

/** [{ line }] of the `aria-busy` uses in code (not in a comment), without a `stream-wait` waiver on the line or the line above. */
export function bareStreamWaits(text) {
  const lines = text.split('\n');
  // Comments are blanked by maskTs together with strings; a string use is told from a comment by the comment-free masked line holding a quote there.
  const masked = maskTs(text).split('\n');
  const out = [];
  lines.forEach((line, i) => {
    for (const m of line.matchAll(/aria-busy/g)) {
      // The use is code when maskTs left a quote character at its position's string (blanked inside), i.e. the masked line has a quote before and after.
      const before = masked[i].slice(0, m.index);
      const inString = /['"`]/.test(before) && /['"`]/.test(masked[i].slice(m.index + 'aria-busy'.length));
      if (!inString) continue;
      if (WAIVER.test(line) || WAIVER.test(lines[i - 1] ?? '')) continue;
      out.push({ line: i + 1 });
      break;
    }
  });
  return out;
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
        out.push(SEVERITY('C144', 21, f, hit.line, 'waits on `aria-busy` itself: one expect timeout then bounds a whole streamed answer (runs 8300, 8377)', 'Call `expectStreamDone(stream)` from @decryp7/paddock-e2e (it waits on progress), or, if this is not a stream-finished wait, add `// conformance: stream-wait — <why>`.'));
      }
    }
    return out;
  },
};

// C156 (ER-21 class "axe read colours mid-motion": Pitwall run 8134 read a row half-way into the dark theme; Debrief's a11y.e2e.ts:6 read the
// Inspector's slide-in half-way in debrief.yml runs 8297, 8300, 8373, 8426, 8547, 8556 and 8643). paddock-e2e's `withPageStill` holds the page
// still for a scan (no transitions, finite animations at their end, reduced motion), and `axeViolations` scans through it. An axe scan in an e2e
// spec or helper runs through one of them. Flagged, in a TypeScript file under `apps/<app>/app/e2e/`, `ui/e2e/` or the kit
// (`ui/projects/paddock-e2e/`) that names AxeBuilder: a call of `.analyze(` that is not inside the argument list of a `withPageStill(` call
// (code only: comments and string contents are masked). A site that must scan a moving page on purpose carries
// `// conformance: axe-still — <why>` on its line or the line above. Limit: no parser; a builder passed to a function that calls `.analyze()`
// elsewhere is judged where `.analyze(` stands.
// ER-41: report-only (warnings) until its first green run on the real tree, recorded by run id here. Recorded: not yet.
import { warn } from '../finding.mjs';
import { maskTs } from './C71-test-timing.mjs';

export const SEVERITY = warn;
const FILE = /^(?:apps\/[^/]+\/app\/e2e\/.+|ui\/e2e\/.+|ui\/projects\/paddock-e2e\/.+)\.ts$/;
const WAIVER = /conformance:\s*axe-still\s*[—–-]+\s*\S.{2,}/;
const STILL = 'withPageStill(';

/** True when position `at` of `code` (masked) lies inside the argument list of the nearest `withPageStill(` call before it. */
function insideStill(code, at) {
  const open = code.lastIndexOf(STILL, at);
  if (open < 0) return false;
  let depth = 0;
  for (let i = open + STILL.length - 1; i < at; i++) {
    if (code[i] === '(') depth++;
    else if (code[i] === ')') {
      depth--;
      if (depth === 0) return false;
    }
  }
  return depth > 0;
}

/** [{ line }] of the `.analyze(` calls in code that do not run inside `withPageStill(` and carry no `axe-still` waiver. */
export function scansNotHeldStill(text) {
  const lines = text.split('\n');
  const code = maskTs(text);
  const out = [];
  for (const m of code.matchAll(/\.analyze\s*\(/g)) {
    if (insideStill(code, m.index)) continue;
    const line = code.slice(0, m.index).split('\n').length - 1;
    if (WAIVER.test(lines[line]) || WAIVER.test(lines[line - 1] ?? '')) continue;
    out.push({ line: line + 1 });
  }
  return out;
}

export default {
  id: 'C156',
  scope: 'root',
  mode: 'report-only',
  title: 'An axe scan in an e2e spec runs with the page held still (withPageStill or axeViolations)',
  rule: 21,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!FILE.test(f)) continue;
      const text = ctx.read(f);
      if (!text || !text.includes('AxeBuilder')) continue;
      for (const hit of scansNotHeldStill(text)) {
        out.push(SEVERITY('C156', 21, f, hit.line, 'runs an axe scan without holding the page still: a transition or animation running during the scan reads as low contrast (Pitwall run 8134, Debrief runs 8297 to 8643)', 'Use `axeViolations(page, options)` from @decryp7/paddock-e2e, or wrap the scan: `withPageStill(page, () => builder.analyze())`; if the page must move during the scan, add `// conformance: axe-still — <why>`.'));
      }
    }
    return out;
  },
};

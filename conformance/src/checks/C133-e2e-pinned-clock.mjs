// C133 (ER-21 class "time-of-day-dependent e2e on a clock-relative mock": Spotter's first-load spec failed just after midnight, CI run
// 8154; Dyno's Issues day headings before it). A mock backend stamps its items relative to the page clock and the app groups them by local
// calendar day, so a spec run just after midnight saw no "Today". `pinnedClockTest` (ui/projects/paddock-e2e, `src/pinned-clock.ts`)
// installs one fixed instant, zone and locale on every page; each app wraps its `test` once in `e2e/test.ts` and its specs import from there.
// Two rules, JS host only (no Rust mock backends exist):
// - rule 1 (the wrapper): a spec under `apps/<app>/app/e2e/` or `ui/e2e/` imports `test` as a value from `@playwright/test` or
//   `@decryp7/paddock-e2e` instead of the app's `e2e/test.ts`. Only the wrapper (`e2e/test.ts`) may; a `import type` is fine, and so are the
//   other names (`expect`, `Page`, the kit's helpers). Playwright configs are not specs.
// - rule 2 (the mock's clock): a mock-backend source file (a path with a `mock` folder or a `mock` word in the file name, not a spec) reads the
//   wall clock itself: `Date.now()` or `new Date()` with no argument. Take `now` from the mock's injected clock (Debrief's `MockClock`,
//   `core/mock/mock-clock.ts`, the one place that may read it) or a `now` option, so a page clock moves every stamp together.
// A site that is fine carries `// conformance: mock-clock — <why>` (rule 2) or `// conformance: e2e-test — <why>` (rule 1) on the line or
// the line above.
// ER-41: report-only (warnings) until its first green run on the real tree is recorded here. Recorded: not yet. The existing mock sites
// are warnings to be moved onto the injected clocks one app at a time.
import { warn } from '../finding.mjs';
import { maskTs } from './C71-test-timing.mjs';

export const SEVERITY = warn;
/** The files that may read the wall clock: the mock clock helpers. */
export const CLOCK_HELPERS = new Set(['apps/debrief/app/src/app/core/mock/mock-clock.ts']);
const WAIVER_1 = /conformance:\s*e2e-test\s*[—–-]+\s*\S.{2,}/;
const WAIVER_2 = /conformance:\s*mock-clock\s*[—–-]+\s*\S.{2,}/;
const SPEC_DIR = /^(?:apps\/[^/]+\/app|ui)\/e2e\/.+\.ts$/;
const WRAPPER = /^(?:apps\/[^/]+\/app|ui)\/e2e\/test\.ts$/;
const CONFIG = /(?:^|\/)playwright[\w.-]*\.config\.ts$/;
const MOCK_FILE = /(?:^|\/)mocks?\/|(?:^|\/)[\w.-]*mock[\w.-]*\.ts$/i;
const SKIP = /(?:^|\/)(?:node_modules|dist|target|vendor)\/|(?:^|\/)fixtures?\/|\.(?:spec|test|e2e|shot)\.ts$|(?:^|\/)testing\//;
const IMPORT = /\bimport\s+(type\s+)?\{([^}]*)\}\s*from\s*(['"])(@playwright\/test|@decryp7\/paddock-e2e)\3/g;
const CLOCK_READ = /\bDate\.now\s*\(\s*\)|\bnew\s+Date\s*\(\s*\)/g;

const lineOf = (text, index) => {
  let line = 1;
  for (let i = 0; i < index; i++) if (text.charCodeAt(i) === 10) line++;
  return line;
};
const waived = (lines, line, re) => re.test(lines[line - 1] ?? '') || re.test(lines[line - 2] ?? '');

/** [{ line, what }] of the value imports of `test` in a spec file (comments masked). */
export function directTestImports(text) {
  const out = [];
  const masked = maskTs(text);
  const lines = text.split('\n');
  for (const m of text.matchAll(IMPORT)) {
    // masked keeps the `import` keyword only where it is code, not in a comment or a string.
    if (m[1] || masked.slice(m.index, m.index + 6) !== 'import') continue;
    // `{ expect, test as base }`: the name before any `as` is what is imported.
    const names = m[2].split(',').map((n) => n.trim().replace(/^type\s+/, '').split(/\s+as\s+/)[0]);
    if (!names.includes('test')) continue;
    const line = lineOf(text, m.index);
    if (waived(lines, line, WAIVER_1)) continue;
    out.push({ line, what: `imports \`test\` from ${m[4]} instead of the app's e2e/test.ts (the page clock is not pinned there)` });
  }
  return out;
}

/** [{ line, what }] of the wall-clock reads in a mock source file (comments and strings masked). */
export function mockClockReads(text) {
  const out = [];
  const masked = maskTs(text);
  const lines = text.split('\n');
  for (const m of masked.matchAll(CLOCK_READ)) {
    const line = lineOf(text, m.index);
    if (waived(lines, line, WAIVER_2)) continue;
    out.push({ line, what: `reads the wall clock (\`${m[0].replace(/\s+/g, '')}\`) in a mock backend; take \`now\` from the mock's injected clock` });
  }
  return out;
}

export default {
  id: 'C133',
  scope: 'root',
  mode: 'report-only',
  title: 'E2E pages run on the pinned clock and mocks take their time from an injected clock',
  rule: 21,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!f.endsWith('.ts')) continue;
      if (SPEC_DIR.test(f) && !WRAPPER.test(f) && !CONFIG.test(f)) {
        const text = ctx.read(f);
        if (text && text.includes('@')) {
          for (const s of directTestImports(text)) {
            out.push(SEVERITY('C133', 21, f, s.line, `${s.what} (rule 1, pinnedClockTest)`, 'Import `test` (and `expect`) from the app\'s e2e/test.ts, which wraps `pinnedClockTest` from @decryp7/paddock-e2e. A spec that truly needs the real time says `test.use({ pinnedClock: false })` with its reason; a helper that only needs types uses `import type`.'));
          }
        }
        continue;
      }
      if (MOCK_FILE.test(f) && !SKIP.test(f) && !CLOCK_HELPERS.has(f) && /^(?:apps\/[^/]+\/app\/src|ui\/projects)\//.test(f)) {
        const text = ctx.read(f);
        if (!text || !/Date/.test(text)) continue;
        for (const s of mockClockReads(text)) {
          out.push(SEVERITY('C133', 21, f, s.line, `${s.what} (rule 2)`, 'Read time through the mock\'s clock (Debrief: `MockClock` in core/mock/mock-clock.ts) or a `now` option, so one page clock moves every stamp; a site that is fine carries `// conformance: mock-clock — <why>`.'));
        }
      }
    }
    return out;
  },
};

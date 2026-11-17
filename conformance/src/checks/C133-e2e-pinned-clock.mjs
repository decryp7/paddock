// C133 (ER-21 class "time-of-day-dependent e2e on a clock-relative mock": Spotter's first-load spec failed just after midnight, CI run
// 8154; Dyno's Issues day headings before it; and the specs that read a page before it settled once a slower fake-clock timer showed them).
// `pinnedClockTest` (ui/projects/paddock-e2e, `src/pinned-clock.ts`) installs one fixed instant, zone and locale on every page and reports a
// throw from a timer callback; each app wraps its `test` once in `e2e/test.ts`. What ESLint can say is ESLint's: the e2e preset
// (`e2ePinnedClock` in eslint/src/e2e.cjs, `no-restricted-imports` with allowTypeImports and `no-restricted-syntax` selectors) refuses a value
// import, default import, namespace import or re-export of `test` from Playwright or the kit, a dynamic import() or require() of them, a bare
// `clock.install()` in any form and `browser.newContext()` / `browser.newPage()` (ER-37: common practice for a syntactic ban is the linter).
// In a spec under `apps/<app>/app/e2e/` or `ui/e2e/` (not `e2e/test.ts`, not Playwright configs), C133 keeps what ESLint cannot express:
// - rule 1 (an opt-out says why): `pinnedClock: false` and `newPinnedContext(..., { pin: false })` carry the waiver form
//   `pinned-clock: off — <why>` (a comment on the same line, `//` or `/* */`, or on the line above), with a reason of at least ten characters.
// - rule 2 (live specs): a spec that imports `pauseLive`, `withLivePaused`, `clickWithLivePaused` or `resumeLive` from the kit is a live-page
//   spec; in each of its tests the first click, key, focus, tap or fill is inside a `withLivePaused(` callback or after a `pauseLive(`, or the
//   test carries `// conformance: e2e-live — <why>` (it drives the updates itself, or reads nothing the updates change).
// - rule 3 (no wait over an empty set, ER-12): a `waitForFunction(` whose function calls `.every(` and checks no length or size of the set
//   (a `.length` inside the `every` callback is the element's own and does not count) is true before the first box is drawn: `every()` over
//   no boxes. This is about boxes that must exist (rows, tables, path boxes); wait with `settledLayout` from the kit (never vacuous), or
//   check the count. Where an empty set IS the settled state (`document.getAnimations()`), the site carries
//   `// conformance: e2e-wait — <why>`. Limit: only a function written inside the call is read; one passed by name is not.
// Out of scope, with their own Playwright configs outside the e2e folders: Debrief's `scripts/screenshots/capture.ts` and
// `scripts/visual-qa/sweep.ts`.
// ER-41: report-only (warnings) until its first green run on the real tree is recorded here. Recorded: not yet.
import { warn } from '../finding.mjs';
import { maskTs } from './C71-test-timing.mjs';

export const SEVERITY = warn;
const OPT_OUT = /pinned-clock:\s*off\s*[—–-]+\s*\S.{9,}/;
const LIVE_WAIVER = /conformance:\s*e2e-live\s*[—–-]+\s*\S.{2,}/;
const WAIT_WAIVER = /conformance:\s*e2e-wait\s*[—–-]+\s*\S.{2,}/;
const SPEC_DIR = /^(?:apps\/[^/]+\/app|ui)\/e2e\/.+\.ts$/;
const WRAPPER = /^(?:apps\/[^/]+\/app|ui)\/e2e\/test\.ts$/;
const CONFIG = /(?:^|\/)playwright[\w.-]*\.config\.ts$/;
const LIVE_NAMES = '(?:pauseLive|withLivePaused|clickWithLivePaused|resumeLive)';
const LIVE_IMPORT = new RegExp(`\\bimport\\s*\\{[^}]*\\b${LIVE_NAMES}\\b[^}]*\\}\\s*from\\s*['"][^'"]+['"]`);

const lineOf = (text, index) => {
  let line = 1;
  for (let i = 0; i < index; i++) if (text.charCodeAt(i) === 10) line++;
  return line;
};
/** The index just past the `)` that closes the `(` at `open` in `code` (comments and strings masked), or -1. */
function closeParen(code, open) {
  let depth = 0;
  for (let i = open; i < code.length; i++) {
    if (code[i] === '(') depth++;
    else if (code[i] === ')' && --depth === 0) return i + 1;
  }
  return -1;
}

/** [{ line, what }] of `pinnedClock: false` and `{ pin: false }` without the waiver form on the line or the line above. */
export function bareOptOuts(text) {
  const out = [];
  const lines = text.split('\n');
  for (const m of maskTs(text).matchAll(/\b(pinnedClock|pin)\s*:\s*false\b/g)) {
    const line = lineOf(text, m.index);
    // `//` and `/* */` comments are read from the unmasked line: the waiver form is the reason.
    if (!OPT_OUT.test(lines[line - 1] ?? '') && !OPT_OUT.test(lines[line - 2] ?? '')) {
      out.push({ line, what: `\`${m[1]}: false\` has no \`pinned-clock: off — <why>\` reason (ten characters or more) on the line or the line above` });
    }
  }
  return out;
}

const INTERACTION = /\.(?:click|dblclick|press|focus|fill|hover|check|uncheck|selectOption|type|tap|pressSequentially)\s*\(|\bkeyboard\.\w+\s*\(/;
const TEST_HEAD = /\btest(?:\.(?:only|fixme|skip|fail))?\s*\(\s*(['"`])(?:\\.|(?!\1)[^\\])*\1\s*,\s*(?:\{[^}]*\}\s*,\s*)?(?:async\s*)?(?:\([^)]*\)|\w+)\s*=>\s*\{/g;

/** [{ line, what }] of a test in a live-page spec whose first interaction is neither inside `withLivePaused(` nor after `pauseLive(`. */
export function liveSteps(text) {
  if (!LIVE_IMPORT.test(text)) return [];
  const out = [];
  const lines = text.split('\n');
  const code = maskTs(text);
  for (const m of text.matchAll(TEST_HEAD)) {
    if (code.slice(m.index, m.index + 4) !== 'test') continue;
    const open = m.index + m[0].length - 1;
    let depth = 0;
    let end = -1;
    for (let i = open; i < code.length; i++) {
      if (code[i] === '{') depth++;
      else if (code[i] === '}' && --depth === 0) {
        end = i;
        break;
      }
    }
    if (end < 0) continue;
    const body = code.slice(open, end);
    const first = INTERACTION.exec(body);
    if (!first) continue;
    // `pauseLive(` before it, or the interaction inside the parentheses of a `withLivePaused(` / `clickWithLivePaused(` call.
    const pauseAt = /\bpauseLive\s*\(/.exec(body);
    let covered = pauseAt !== null && pauseAt.index < first.index;
    for (const w of body.matchAll(/\b(?:withLivePaused|clickWithLivePaused)\s*\(/g)) {
      const from = w.index + w[0].length - 1;
      const to = closeParen(body, from);
      if (first.index > from && (to < 0 || first.index < to)) covered = true;
    }
    if (covered) continue;
    const line = lineOf(text, open + first.index);
    const headLine = lineOf(text, m.index);
    if ([headLine, headLine - 1, line, line - 1].some((l) => LIVE_WAIVER.test(lines[l - 1] ?? ''))) continue;
    out.push({ line, what: 'the first click, key, tap or fill of a live-page test is not inside withLivePaused( and not after pauseLive( (a live update can overtake it)' });
  }
  return out;
}

/** [{ line, what }] of `waitForFunction(` calls whose function calls `.every(` and checks no length or size (comments and strings masked). */
export function emptySetWaits(text) {
  const out = [];
  const lines = text.split('\n');
  const code = maskTs(text);
  for (const m of code.matchAll(/\bwaitForFunction\s*\(/g)) {
    const open = m.index + m[0].length - 1;
    const close = closeParen(code, open);
    if (close < 0) continue;
    const call = code.slice(open, close);
    const every = /\.every\s*\(/.exec(call);
    if (!every) continue;
    // A length or size outside the `every` callback is a check of the set; inside it, the element's own.
    const cbOpen = every.index + every[0].length - 1;
    const cbClose = closeParen(call, cbOpen);
    const outside = call.slice(0, cbOpen) + (cbClose < 0 ? '' : call.slice(cbClose));
    if (/\.length\b|\.size\b|\.count\b/.test(outside)) continue;
    const line = lineOf(text, m.index);
    if (WAIT_WAIVER.test(lines[line - 1] ?? '') || WAIT_WAIVER.test(lines[line - 2] ?? '')) continue;
    out.push({ line, what: '`waitForFunction` over `.every(` with no length check is true before any box is drawn (every() over an empty set)' });
  }
  return out;
}

const FIX = {
  1: 'Put `// pinned-clock: off — <why the spec needs the real time>` (ten characters or more) on the line or the line above `pinnedClock: false` / `{ pin: false }`.',
  2: 'Call `await pauseLive(page)` before the first interaction, wrap the steps in `withLivePaused(page, ...)` or use `clickWithLivePaused`; a test that drives the live updates itself, or reads nothing they change, carries `// conformance: e2e-live — <why>`.',
  3: 'Wait with `settledLayout` from @decryp7/paddock-e2e (two agreeing reads, never vacuous), or check the count before `every()`; a site that is fine carries `// conformance: e2e-wait — <why>`.',
};

export default {
  id: 'C133',
  scope: 'root',
  mode: 'report-only',
  title: 'E2E specs: opt-outs of the pinned clock say why, live steps are paused, waits are never vacuous',
  rule: 21,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!f.endsWith('.ts') || !SPEC_DIR.test(f) || WRAPPER.test(f) || CONFIG.test(f)) continue;
      const text = ctx.read(f);
      if (!text) continue;
      const found = [
        ...(/pinnedClock|\bpin\s*:\s*false/.test(text) ? bareOptOuts(text).map((s) => ({ ...s, rule: 1 })) : []),
        ...liveSteps(text).map((s) => ({ ...s, rule: 2 })),
        ...(text.includes('waitForFunction') ? emptySetWaits(text).map((s) => ({ ...s, rule: 3 })) : []),
      ];
      for (const s of found) out.push(SEVERITY('C133', 21, f, s.line, `${s.what} (rule ${s.rule})`, FIX[s.rule]));
    }
    return out;
  },
};

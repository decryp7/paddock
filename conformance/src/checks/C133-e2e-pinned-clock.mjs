// C133 (ER-21 class "time-of-day-dependent e2e on a clock-relative mock": Spotter's first-load spec failed just after midnight, CI run
// 8154; Dyno's Issues day headings before it). `pinnedClockTest` (ui/projects/paddock-e2e, `src/pinned-clock.ts`) installs one fixed
// instant, zone and locale on every page, and reports a throw from a timer callback; each app wraps its `test` once in `e2e/test.ts`
// and its specs import from there. In the page `Date.now()` and `new Date()` already are the pinned clock, so a mock's own clock read
// moves with the pin and is not this check's business (reviewed 2026-11-16). JS host only. Three rules over the specs under
// `apps/<app>/app/e2e/` and `ui/e2e/` (not the wrapper `e2e/test.ts`, not Playwright configs):
// - rule 1 (the wrapper): a spec reaches Playwright's or the kit's `test` other than through the app's e2e/test.ts: a value import of `test`
//   (also `test as x`), a default import, a namespace import (`import * as pw`), or a re-export (`export { test } from`, `export * from`)
//   of `@playwright/test` or `@decryp7/paddock-e2e`, or a dynamic `import('...')` / `require('...')` of either. `import type`, `expect`,
//   `Page` and the kit's other helpers are fine.
// - rule 2 (the opt-out says why): `pinnedClock: false` and `newPinnedContext(..., { pin: false })` carry a reason (a comment of at least
//   three words, or `// conformance: e2e-test — <why>`) on the same line or the line above.
// - rule 3 (pages the fixture does not pin): a bare `clock.install()`, `clock.install({})` or `clock.install(undefined)` (it restarts the
//   clock at the real time; the pin is already installed, pass `{ time }` or drop it), a `browser.newContext(` or a `browser.newPage(` (a
//   context the fixtures did not make is not pinned: use the kit's `newPinnedContext`).
// - rule 4 (live specs): a spec that imports `pauseLive`, `withLivePaused`, `clickWithLivePaused` or `resumeLive` is a live-page spec; in each of its tests the first
//   click, key, focus or fill goes after `pauseLive(` or inside `withLivePaused(`, so a live update does not overtake the step (a row menu
//   opened, then the rows redrawn under it). A test that drives the updates itself carries `// conformance: e2e-live — <why>`.
// A site that is fine carries `// conformance: e2e-test — <why>` on the line or the line above (rules 1 to 3), `e2e-live` for rule 4.
// Out of scope, with their own Playwright configs outside the e2e folders: Debrief's `scripts/screenshots/capture.ts` and
// `scripts/visual-qa/sweep.ts`.
// ER-41: report-only (warnings) until its first green run on the real tree is recorded here. Recorded: not yet.
import { warn } from '../finding.mjs';
import { maskTs } from './C71-test-timing.mjs';

export const SEVERITY = warn;
const WAIVER = /conformance:\s*e2e-test\s*[—–-]+\s*\S.{2,}/;
const LIVE_WAIVER = /conformance:\s*e2e-live\s*[—–-]+\s*\S.{2,}/;
/** A reason: the waiver form, or a comment of at least three words. */
const REASON = /conformance:\s*e2e-test\s*[—–-]+\s*\S.{2,}|\/\/\s*\S+(?:\s+\S+){2,}/;
const SPEC_DIR = /^(?:apps\/[^/]+\/app|ui)\/e2e\/.+\.ts$/;
const WRAPPER = /^(?:apps\/[^/]+\/app|ui)\/e2e\/test\.ts$/;
const CONFIG = /(?:^|\/)playwright[\w.-]*\.config\.ts$/;
const MODULES = '@playwright/test|@decryp7/paddock-e2e';
// A statement that imports from or re-exports one of the two modules: the clause, then the module (strings are kept in `text`).
const STATEMENT = new RegExp(`\\b(import|export)\\s+(type\\s+)?([^;'"]*?)\\s*from\\s*(['"])(${MODULES})\\4`, 'g');

const lineOf = (text, index) => {
  let line = 1;
  for (let i = 0; i < index; i++) if (text.charCodeAt(i) === 10) line++;
  return line;
};
const waived = (lines, line) => WAIVER.test(lines[line - 1] ?? '') || WAIVER.test(lines[line - 2] ?? '');
const withReason = (lines, line) => REASON.test(lines[line - 1] ?? '') || REASON.test(lines[line - 2] ?? '');

/** What a clause brings in by value: `test`, a default, a namespace, or everything (`export *`). Null when it brings no `test`. */
function reach(kind, clause) {
  const c = clause.trim();
  if (kind === 'export' && c.startsWith('*')) return 'a re-export of everything (`export *`)';
  if (c.startsWith('* as')) return 'a namespace import (it can reach `.test`)';
  const named = /\{([^}]*)\}/.exec(c);
  const outside = c.replace(/\{[^}]*\}/, '').replace(/,/g, '').trim();
  if (kind === 'import' && outside) return 'a default import';
  if (named) {
    const names = named[1].split(',').map((n) => n.trim().replace(/^type\s+/, '').split(/\s+as\s+/)[0]);
    if (names.includes('test')) return kind === 'export' ? 'a re-export of `test`' : 'a value import of `test`';
  }
  return null;
}

/** [{ line, what }] of the ways a spec reaches `test` outside the wrapper (comments masked). */
export function directTestImports(text) {
  const out = [];
  const masked = maskTs(text);
  const lines = text.split('\n');
  for (const m of text.matchAll(STATEMENT)) {
    // masked keeps the keyword only where it is code, not in a comment or a string.
    if (m[2] || masked.slice(m.index, m.index + m[1].length) !== m[1]) continue;
    const how = reach(m[1], m[3]);
    if (!how) continue;
    const line = lineOf(text, m.index);
    if (waived(lines, line)) continue;
    out.push({ line, what: `${how} from ${m[5]} instead of the app's e2e/test.ts (the page clock is not pinned there)` });
  }
  // A value reached through a dynamic import() (awaited, assigned or `.then`) or a require() is not pinned either; `import('x').Page` in a
  // type position is not a value.
  const dynamic = new RegExp(`(?:\\bawait\\s+|=\\s*|\\breturn\\s+)(import)\\s*\\(\\s*(['"])(${MODULES})\\2\\s*\\)(?!\\s*\\.[A-Z])|\\b(require)\\s*\\(\\s*(['"])(${MODULES})\\5\\s*\\)`, 'g');
  for (const m of text.matchAll(dynamic)) {
    const kw = m[1] ?? m[4];
    const at = m.index + m[0].indexOf(kw);
    if (masked.slice(at, at + kw.length) !== kw) continue;
    const line = lineOf(text, at);
    if (waived(lines, line)) continue;
    out.push({ line, what: `a dynamic ${kw}() of ${m[3] ?? m[6]} instead of the app's e2e/test.ts (the page clock is not pinned there)` });
  }
  return out;
}

/** [{ line, what }] of `pinnedClock: false` and `{ pin: false }` without a reason on the line or the line above. */
export function bareOptOuts(text) {
  const out = [];
  const lines = text.split('\n');
  const masked = maskTs(text);
  for (const m of masked.matchAll(/\b(pinnedClock|pin)\s*:\s*false\b/g)) {
    const line = lineOf(text, m.index);
    if (!withReason(lines, line)) out.push({ line, what: `\`${m[1]}: false\` has no reason on the line or the line above (say what needs the real time)` });
  }
  return out;
}

/** [{ line, what }] of a bare `clock.install()` and a `browser.newContext(` (comments and strings masked). */
export function unpinnedPages(text) {
  const out = [];
  const masked = maskTs(text);
  const lines = text.split('\n');
  const find = (re, what) => {
    for (const m of masked.matchAll(re)) {
      const line = lineOf(text, m.index);
      if (!waived(lines, line)) out.push({ line, what });
    }
  };
  find(/\bclock\.install\s*\(\s*(?:\{\s*\}|undefined)?\s*\)/g, 'a bare `clock.install()` (also `({})` and `(undefined)`) restarts the pinned clock at the real time (pass `{ time }`, or drop it: the pin is installed)');
  find(/\bbrowser\.newContext\s*\(/g, '`browser.newContext(` makes a context the fixtures did not pin');
  find(/\bbrowser\.newPage\s*\(/g, '`browser.newPage(` makes a context the fixtures did not pin');
  return out;
}

const INTERACTION = /\.(?:click|dblclick|press|focus|fill|hover|check|uncheck|selectOption|type|pressSequentially)\s*\(|\bkeyboard\.\w+\s*\(/;
const LIVE_PAUSE = /\bpauseLive\s*\(|\bwithLivePaused\s*\(|\bclickWithLivePaused\s*\(/;
const TEST_HEAD = /\btest(?:\.(?:only|fixme|skip|fail))?\s*\(\s*(['"`])(?:\\.|(?!\1)[^\\])*\1\s*,\s*(?:\{[^}]*\}\s*,\s*)?(?:async\s*)?(?:\([^)]*\)|\w+)\s*=>\s*\{/g;

/** [{ line, what }] of a test in a live-page spec whose first interaction is not after `pauseLive(` (or inside `withLivePaused(`). */
export function liveSteps(text) {
  if (!/\b(?:pauseLive|withLivePaused|resumeLive|clickWithLivePaused)\b/.test(text)) return [];
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
    const pause = LIVE_PAUSE.exec(body);
    const at = open + first.index;
    // withLivePaused( wraps the steps: an interaction inside its callback is paused; one before the call is not.
    if (pause && pause.index <= first.index) continue;
    const line = lineOf(text, at);
    const headLine = lineOf(text, m.index);
    if ([headLine, headLine - 1, line, line - 1].some((l) => LIVE_WAIVER.test(lines[l - 1] ?? ''))) continue;
    out.push({ line, what: 'the first click, key or fill of a live-page test comes before `pauseLive(` (a live update can overtake it)' });
  }
  return out;
}

const FIX = {
  1: 'Import `test` (and `expect`) from the app\'s e2e/test.ts, which wraps `pinnedClockTest` from @decryp7/paddock-e2e; a helper that only needs types uses `import type`.',
  2: 'Put a reason (a comment of at least three words) on the line or the line above `pinnedClock: false` / `{ pin: false }` saying what needs the real time (for example a measure of real frame cost).',
  4: 'Call `await pauseLive(page)` (or wrap the steps in `withLivePaused(page, ...)`) before the first interaction; a test that drives the live updates itself carries `// conformance: e2e-live — <why>`.',
  3: 'Drop the bare install (the clock is pinned), pass `{ time }`, or install the pin on the context you create (`context.clock.install({ time })`); a site that is fine carries `// conformance: e2e-test — <why>`.',
};

export default {
  id: 'C133',
  scope: 'root',
  mode: 'report-only',
  title: 'E2E specs run on the pinned page clock',
  rule: 21,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!f.endsWith('.ts') || !SPEC_DIR.test(f) || WRAPPER.test(f) || CONFIG.test(f)) continue;
      const text = ctx.read(f);
      if (!text) continue;
      const found = [
        ...(text.includes('@') ? directTestImports(text).map((s) => ({ ...s, rule: 1 })) : []),
        ...((text.includes('pinnedClock') || /\bpin\s*:\s*false/.test(text)) ? bareOptOuts(text).map((s) => ({ ...s, rule: 2 })) : []),
        ...liveSteps(text).map((s) => ({ ...s, rule: 4 })),
        ...(/clock\.install|newContext|newPage/.test(text) ? unpinnedPages(text).map((s) => ({ ...s, rule: 3 })) : []),
      ];
      for (const s of found) out.push(SEVERITY('C133', 21, f, s.line, `${s.what} (rule ${s.rule}, pinnedClockTest)`, FIX[s.rule]));
    }
    return out;
  },
};

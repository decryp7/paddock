// C133 (ER-21 class "time-of-day-dependent e2e on a clock-relative mock": Spotter's first-load spec failed just after midnight, CI run
// 8154; Dyno's Issues day headings before it). `pinnedClockTest` (ui/projects/paddock-e2e, `src/pinned-clock.ts`) installs one fixed
// instant, zone and locale on every page, and reports a throw from a timer callback; each app wraps its `test` once in `e2e/test.ts`
// and its specs import from there. In the page `Date.now()` and `new Date()` already are the pinned clock, so a mock's own clock read
// moves with the pin and is not this check's business (reviewed 2026-11-16). JS host only. Three rules over the specs under
// `apps/<app>/app/e2e/` and `ui/e2e/` (not the wrapper `e2e/test.ts`, not Playwright configs):
// - rule 1 (the wrapper): a spec reaches Playwright's or the kit's `test` other than through the app's e2e/test.ts: a value import of `test`
//   (also `test as x`), a default import, a namespace import (`import * as pw`), or a re-export (`export { test } from`, `export * from`)
//   of `@playwright/test` or `@decryp7/paddock-e2e`. `import type`, `expect`, `Page` and the kit's other helpers are fine.
// - rule 2 (the opt-out says why): `pinnedClock: false` has a comment on the line above that says why the spec needs the real time.
// - rule 3 (pages the fixture does not pin): a bare `clock.install()` (it restarts the clock at the real time; the pin is already
//   installed, pass `{ time }` or drop it) or a `browser.newContext(` (a context the fixtures did not make is not pinned).
// A site that is fine carries `// conformance: e2e-test — <why>` on the line or the line above (rules 1 and 3).
// Out of scope, with their own Playwright configs outside the e2e folders: Debrief's `scripts/screenshots/capture.ts` and
// `scripts/visual-qa/sweep.ts`.
// ER-41: report-only (warnings) until its first green run on the real tree is recorded here. Recorded: not yet.
import { warn } from '../finding.mjs';
import { maskTs } from './C71-test-timing.mjs';

export const SEVERITY = warn;
const WAIVER = /conformance:\s*e2e-test\s*[—–-]+\s*\S.{2,}/;
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
  return out;
}

/** [{ line, what }] of `pinnedClock: false` without a comment on the line above. */
export function bareOptOuts(text) {
  const out = [];
  const lines = text.split('\n');
  const masked = maskTs(text);
  for (const m of masked.matchAll(/\bpinnedClock\s*:\s*false\b/g)) {
    const line = lineOf(text, m.index);
    const above = (lines[line - 2] ?? '').trim();
    if (!/^\/\/\s*\S.{4,}/.test(above) && !/^\*\s*\S.{4,}|^\/\*\*?\s*\S.{4,}/.test(above)) {
      out.push({ line, what: '`pinnedClock: false` has no reason on the line above (say what needs the real time)' });
    }
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
  find(/\bclock\.install\s*\(\s*\)/g, 'a bare `clock.install()` restarts the pinned clock at the real time (pass `{ time }`, or drop it: the pin is installed)');
  find(/\bbrowser\.newContext\s*\(/g, '`browser.newContext(` makes a context the fixtures did not pin');
  return out;
}

const FIX = {
  1: 'Import `test` (and `expect`) from the app\'s e2e/test.ts, which wraps `pinnedClockTest` from @decryp7/paddock-e2e; a helper that only needs types uses `import type`.',
  2: 'Put a comment on the line above `pinnedClock: false` saying what needs the real time (for example a measure of real frame cost).',
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
        ...(text.includes('pinnedClock') ? bareOptOuts(text).map((s) => ({ ...s, rule: 2 })) : []),
        ...(/clock\.install|newContext/.test(text) ? unpinnedPages(text).map((s) => ({ ...s, rule: 3 })) : []),
      ];
      for (const s of found) out.push(SEVERITY('C133', 21, f, s.line, `${s.what} (rule ${s.rule}, pinnedClockTest)`, FIX[s.rule]));
    }
    return out;
  },
};

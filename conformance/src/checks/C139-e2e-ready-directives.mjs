// C139: the exceptions to `paddock/e2e-ready-reads` only shrink. The rule is an error in every app's and the ui's e2e lint block (so the
// existing `--max-warnings 0` lint steps block a new unready read), and that block sets `noInlineConfig`: no inline comment, in any of ESLint's
// spellings, can switch it off (ESLint's own grammar decides; we do not parse it). The one way to except a read is an entry of the project's
// `eslint-suppressions.json` (ESLint bulk suppressions: `{ "<spec>": { "<rule>": { "count": n } } }`), and the user approves a new one (ER-46).
// ESLint's CLI fails on an entry that no longer occurs (never run with `--pass-on-unpruned-suppressions`; eslint/test/wiring.test.cjs runs it and
// forbids the flag), so a fixed read leaves no slack for a new one. This check is the other half: each entry, counted `count` times, is compared
// with the file at the merge base (baselineGrowth, the one comparison C53 and C115 use; "not measured" is an error in CI, ADR 0089), and an entry
// the merge base lacks, or a raised count, is one unwaivable finding at that file.
//
// Read: the `eslint-suppressions.json` of an app (apps/<app>/app/) and of the ui. A malformed file is one check-broken finding at it. A run that
// finds no e2e spec at all (apps/<app>/app/e2e/*.ts, ui/e2e/*.ts) did not look at anything: one not-measured error, never a pass (ER-33).
//
// Blocking (mode gate, ER-41): its first green CI run that included it was ci.yml run 8335 (train 15, 2026-11-20), and ci.yml run 8869
// (cdd4c3d10, 2026-11-24) ran it clean on the real tree; the switch was made on 2026-11-24 (user), here and in c139.rs. The lint rule itself is an error already: it
// had its report-only phase, and the real tree is at 0 sites outside the suppressions files.
// JavaScript port of tools/paddock-dev/src/check/c139.rs (same findings, line for line; tests/it/check_c139.rs compares them).
import { error, unwaivable } from '../finding.mjs';
import { baselineGrowth, isWorkTree, targetRef, unmeasuredFinding } from '../util/baseline.mjs';

export const RULE_ID = 'paddock/e2e-ready-reads';
const E2E_TS = /^(?:apps\/[^/]+\/app|ui)\/e2e\/.+\.ts$/;
const SUPPRESSIONS = /^(?:apps\/[^/]+\/app|ui)\/eslint-suppressions\.json$/;
const NOT_READ = /(?:^|\/)(?:node_modules|[^/]*-snapshots)\//;
const FIX_GROW = "Wait with a ready helper from @decryp7/paddock-e2e (ui/projects/paddock-e2e/src/ready.ts) instead. An exception is an entry of the project's eslint-suppressions.json, and the file only shrinks: a new entry needs the user's approval (ER-46).";
const FIX_BROKEN = 'Restore the file: ESLint bulk suppressions, `{ "e2e/a.spec.ts": { "paddock/e2e-ready-reads": { "count": 1 } } }` (`npx eslint e2e --suppress-rule paddock/e2e-ready-reads` writes it).';

/** The entries of an `eslint-suppressions.json` object, each as `count` copies of the key `file NUL rule`; null when it is not that shape. */
export function entriesOf(json) {
  if (json === null || typeof json !== 'object' || Array.isArray(json)) return null;
  const out = [];
  for (const [file, rules] of Object.entries(json)) {
    if (rules === null || typeof rules !== 'object' || Array.isArray(rules)) return null;
    for (const [rule, body] of Object.entries(rules)) {
      const count = body?.count;
      if (!Number.isInteger(count) || count < 1 || count > 10000) return null;
      for (let i = 0; i < count; i++) out.push(`${file}\u0000${rule}`);
    }
  }
  return out;
}

export default {
  id: 'C139',
  scope: 'root',
  mode: 'gate',
  title: 'the exceptions to paddock/e2e-ready-reads (eslint-suppressions.json) only shrink',
  rule: 49,
  run(ctx) {
    const files = ctx.files();
    const specs = files.filter((f) => E2E_TS.test(f) && !NOT_READ.test(f)).length;
    if (specs === 0) {
      return [unwaivable(error('C139', 49, 'ui/e2e', 1, 'not measured: the run read no e2e spec (apps/<app>/app/e2e, ui/e2e), so no exception list was looked at', 'Run the check over the whole repository.'), 'not-measured')];
    }
    const out = [];
    for (const f of files.filter((p) => SUPPRESSIONS.test(p))) {
      const text = ctx.read(f);
      if (text === null || text === undefined) {
        out.push(unwaivable(error('C139', 49, f, 1, `not measured: ${f} could not be read, so its exceptions cannot be seen`, 'Make the file readable (permissions) and run the check again.'), 'not-measured'));
        continue;
      }
      let current = null;
      try {
        current = entriesOf(JSON.parse(text));
      } catch {
        current = null;
      }
      if (current === null) {
        out.push(unwaivable(error('C139', 49, f, 1, 'the check cannot look: not ESLint bulk suppressions (an object of files, rules and counts of at least 1)', FIX_BROKEN), 'check-broken'));
        continue;
      }
      if (!ctx.root || !isWorkTree(ctx.root)) continue;
      const g = baselineGrowth({
        root: ctx.root,
        path: f,
        current,
        keyOf: (k) => k,
        entriesOf: (json) => {
          const e = entriesOf(json);
          if (e === null) throw new Error('unparsed');
          return e;
        },
        ...(ctx.options?.git ? { git: ctx.options.git } : {}),
        baseRef: targetRef(ctx.options),
        check: 'C139',
      });
      if (g.reason) {
        out.push(unmeasuredFinding(ctx, { id: 'C139', rule: 49, file: f, what: 'the exception list', g }));
        continue;
      }
      if (g.added.length === 0) continue;
      const n = g.added.length;
      out.push({ ...error('C139', 49, f, 1, `the exception list grew: ${n} entr${n === 1 ? 'y is' : 'ies are'} (counting each \`count\`) not in ${f} at the merge base ${g.base.slice(0, 9)}; it may only shrink`, `${FIX_GROW}${g.ask ? ` ${g.ask}` : ''}`), unwaivable: true });
    }
    return out;
  },
};

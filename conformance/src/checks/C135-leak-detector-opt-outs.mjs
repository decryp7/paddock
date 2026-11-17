// C135: the test leak detector is switched off or narrowed only where a list says so (ER-21 class "a test fails for what an earlier
// test left behind"; ER-46: lists only shrink). `installLeakDetector({ mode: 'off' })` judges nothing and `{ skip: [...] }` drops
// whole kinds of check, so each is a hole in the detector. Flagged: a call of `installLeakDetector(` in a `.ts` file under `apps/` or
// `ui/` whose argument object has `mode: 'off'` or a `skip:` key (the call's own text up to its closing parenthesis), unless the file
// has an entry in `conformance/c135-allow.json` (a `path`, a `site` (`mode: 'off'` or `skip:`), a `reason` and a `count`, read by the
// shared allow list). The list is held to shrink-only against the merge base with the target branch (`util/baseline.mjs`): an entry
// added by a branch is an error. A file that cannot be read is not measured (an error finding at that file); an allow file that cannot be
// read makes the check broken (one finding, nothing else).
// Report-only (warnings) until its first green run on the real tree is recorded here (ER-41). Recorded: not yet.
import { error, warn } from '../finding.mjs';
import { baselineGrowth } from '../util/baseline.mjs';
import { parseAllowList, readAllowList } from '../util/allow-list.mjs';

export const ALLOW = 'conformance/c135-allow.json';
const TS = /^(?:apps|ui)\/.*\.ts$/;
const SKIP_DIRS = /(?:^|\/)(?:node_modules|dist)\//;
const CALL = /\binstallLeakDetector\s*\(/g;
const FIX = 'Fix the leak the suite makes (clear the timer, remove the node, reset the singleton) instead of switching the detector off; a suite that leaks on purpose, to test the detector or the kit, gets an entry in conformance/c135-allow.json with a reason.';

/** [{ line, site }] of every opt-out in `text`: the argument of each `installLeakDetector(` call, up to its closing parenthesis. */
export function optOuts(text) {
  const out = [];
  if (!text.includes('installLeakDetector')) return out;
  for (const m of text.matchAll(CALL)) {
    let depth = 1;
    let i = m.index + m[0].length;
    const start = i;
    for (; i < text.length && depth > 0; i++) {
      if (text[i] === '(') depth++;
      else if (text[i] === ')') depth--;
    }
    const args = text.slice(start, i);
    const line = text.slice(0, m.index).split('\n').length;
    if (/\bmode\s*:\s*['"]off['"]/.test(args)) out.push({ line, site: "mode: 'off'" });
    if (/\bskip\s*:/.test(args)) out.push({ line, site: 'skip:' });
  }
  return out;
}

/** The allow list: [{ path, site, reason, count }], or throws why it cannot be read. */
export const parseAllow = (text) => parseAllowList(text, ALLOW);
const SPEC = { id: 'C135', rule: 21, file: ALLOW, unseen: 'a leak detector opt-out', restore: 'Restore conformance/c135-allow.json: {"allow": [{"path": "...", "site": "mode: \'off\'", "reason": "...", "count": 1}]}.' };

export default {
  id: 'C135',
  scope: 'root',
  mode: 'report-only',
  allowList: SPEC,
  title: 'The test leak detector is switched off only where listed',
  rule: 21,
  run(ctx) {
    const { list: allow, broken } = readAllowList(ctx, this.allowList);
    if (broken) return broken;
    const out = [];
    for (const f of ctx.files()) {
      if (!TS.test(f) || SKIP_DIRS.test(f)) continue;
      const { text, finding } = allow.read(f);
      if (finding) {
        out.push(finding);
        continue;
      }
      for (const { line, site } of optOuts(text)) {
        if (allow.admit(f, (s) => s === site) === 'allowed') continue;
        out.push(warn('C135', 21, f, line, `switches the leak detector off with \`${site}\` and is not in ${ALLOW}`, FIX));
      }
    }
    // An empty list cannot have grown: nothing to measure.
    const g = allow.entries.length === 0 ? { added: [] } : baselineGrowth({
      root: ctx.root,
      path: ALLOW,
      current: allow.entries,
      keyOf: (e) => `${e.path}\0${e.site}`,
      entriesOf: (j) => j.allow ?? [],
      ...(ctx.options.git ? { git: ctx.options.git } : {}),
      ...(ctx.options.baseRef ? { baseRef: ctx.options.baseRef } : {}),
    });
    if (g.reason) out.push(warn('C135', 21, ALLOW, 1, `the list's shrink-only rule is not enforced here: ${g.reason}`, 'Fetch the target branch (`git fetch origin main`), or set PADDOCK_BASE_REF.'));
    else for (const a of g.added) out.push(error('C135', 21, ALLOW, 1, `the list grew: \`${a.site}\` in ${a.path} is not in the list at the merge base ${g.base.slice(0, 9)}`, "Remove the entry and fix the suite; growing the list needs the user's approval."));
    return [...out, ...allow.warnings()];
  },
};

// C140: every `// per-item: <reason>` opt-out of the ESLint rule `paddock/no-backend-call-in-loop` (eslint/src/rules; PRINCIPLES, Bulk
// APIs; the bulk-API redesign's G5) is counted in a list that only shrinks (ER-46), the way C135 counts the leak detector's opt-outs. An
// opt-out keeps one backend call per item on purpose, so each is a cost someone accepted: the list makes the total visible and growth a
// decision. Flagged, in a `.ts` file under `apps/` or `ui/` (specs, e2e and node_modules skipped): an opt-out comment (`//` or `/* */`)
// whose file has no entry in `conformance/c140-allow.json` (a `path`, the site `per-item`, a `reason` and a `count`, read by the shared
// allow list), and one whose reason has fewer than three words (the lint refuses it too). The shared allow list holds it to shrink-only against
// the merge base with the target branch (`util/allow-list.mjs`, through `util/baseline.mjs`): an entry added by a branch is a finding. A file that cannot be read is not
// measured; an allow file that cannot be read makes the check broken (one finding, nothing else).
// Report-only (warnings) until its first green run on the real tree is recorded here (ER-41). Recorded: not yet.
import { warn } from '../finding.mjs';
import { parseAllowList, readAllowList } from '../util/allow-list.mjs';

export const ALLOW = 'conformance/c140-allow.json';
const TS = /^(?:apps|ui)\/.*\.ts$/;
const SKIP = /(?:^|\/)(?:node_modules|dist|e2e)\/|\.spec\.ts$/;
/** An opt-out comment: `//` or `/*`, then `per-item:` and the reason up to the end of the line or the comment. */
const OPT_OUT = /(?:\/\/|\/\*)\s*per-item:[ \t]*([^\n]*?)\s*(?:\*\/|$)/gm;
/** The shortest reason the lint takes: three words. */
export const MIN_WORDS = 3;
const FIX = 'Add a bulk command that takes every item (PRINCIPLES, Bulk APIs) and remove the opt-out; an opt-out that must stay gets an entry in conformance/c140-allow.json (path, site "per-item", reason, count), which needs the user\'s yes (ER-46).';

/** [{ line, reason, short }] of every per-item opt-out comment in `text`. */
export function optOuts(text) {
  const out = [];
  if (!text.includes('per-item:')) return out;
  for (const m of text.matchAll(OPT_OUT)) {
    const reason = m[1].trim();
    const line = text.slice(0, m.index).split('\n').length;
    out.push({ line, reason, short: reason.split(/\s+/).filter(Boolean).length < MIN_WORDS });
  }
  return out;
}

/** The allow list: [{ path, site, reason, count }], or throws why it cannot be read. */
export const parseAllow = (text) => parseAllowList(text, ALLOW);
const SPEC = { id: 'C140', rule: 21, file: ALLOW, unseen: 'a per-item opt-out', restore: 'Restore conformance/c140-allow.json: {"allow": [{"path": "...", "site": "per-item", "reason": "...", "count": 1}]}.' };

export default {
  id: 'C140',
  scope: 'root',
  mode: 'report-only',
  allowList: SPEC,
  title: 'Every per-item opt-out of the bulk-call lint is counted in a list that only shrinks',
  rule: 21,
  run(ctx) {
    const { list: allow, broken } = readAllowList(ctx, this.allowList);
    if (broken) return broken;
    const out = [];
    for (const f of ctx.files()) {
      if (!TS.test(f) || SKIP.test(f)) continue;
      const { text, finding } = allow.read(f);
      if (finding) {
        out.push(finding);
        continue;
      }
      for (const { line, reason, short } of optOuts(text)) {
        if (short) out.push(warn('C140', 21, f, line, `a per-item opt-out with a reason of fewer than ${MIN_WORDS} words (\`${reason}\`)`, 'Say why this call must stay one per item, in a sentence.'));
        if (allow.admit(f, (s) => s === 'per-item') === 'allowed') continue;
        out.push(warn('C140', 21, f, line, `keeps a backend call per item (\`// per-item:\`) and is not in ${ALLOW}`, FIX));
      }
    }
    // The shared allow list compares the list with the merge base itself (its growth findings come with `warnings()`).
    return [...out, ...allow.warnings()];
  },
};

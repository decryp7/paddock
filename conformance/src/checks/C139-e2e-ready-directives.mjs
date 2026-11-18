// C139: an e2e spec that switches off `paddock/e2e-ready-reads` says why, and the exceptions only shrink. The ESLint rule (an error in every
// app's and the ui's e2e lint block, so the existing `--max-warnings 0` lint steps block a new unready read) has one escape, ESLint's own
// `// eslint-disable-next-line paddock/e2e-ready-reads -- <why>`. ESLint cannot require the description or count the directives; this check does
// (ER-21 class "a read on 'it is in the DOM'", ER-46: a list of exceptions only shrinks). `reportUnusedDisableDirectives` (ESLint, on by default
// as a warning, which the lint steps block) removes a directive whose read was fixed.
//
// Flagged, one finding per line, in `.ts` files of an `e2e` folder of an app or the ui:
//  - a `eslint-disable` directive that names no rule (it would switch off everything: say which rule);
//  - the block form (no `-next-line` or `-line`): it covers any number of reads;
//  - a directive for the rule without a description (`-- ` and at least three characters);
//  - a directive that the allow list `conformance/c139-allow.json` does not name, or one beyond its entry's `count`.
// The list is read by the shared reader (util/allow-list.mjs): it only shrinks against the merge base, a stale entry is a warning.
// Report-only (warnings) until its first green CI run that includes it (ER-41); it turns blocking then (mode gate, and `--strict C139`), with that run id recorded here and in c139.rs.
// Recorded: not yet. The lint rule itself is an error already: it had its report-only phase, and the real tree is at 0.
// JavaScript port of tools/paddock-dev/src/check/c139.rs (same findings, line for line; tests/it/check_c139.rs compares them).
import { warn } from '../finding.mjs';
import { parseAllowList, readAllowList } from '../util/allow-list.mjs';

export const ALLOW = 'conformance/c139-allow.json';
export const RULE_ID = 'paddock/e2e-ready-reads';
/** The check's own files: they name the pattern, their invented lines are test data. */
export const IMPLEMENTATION = new Set([
  'conformance/src/checks/C139-e2e-ready-directives.mjs',
  'conformance/test/c139-e2e-ready-directives.test.mjs',
  'tools/paddock-dev/src/check/c139.rs',
  'tools/paddock-dev/tests/it/check_c139.rs',
  ALLOW,
]);
const E2E_TS = /^(?:apps\/[^/]+\/app|ui)\/e2e\/.+\.ts$/;
const NOT_READ = /(?:^|\/)(?:node_modules|[^/]*-snapshots)\//;
const DIRECTIVE = /(?:\/\/|\/\*)\s*eslint-disable(-next-line|-line)?\b(.*)$/;
const FIX_RULES = 'Name the rule: `// eslint-disable-next-line paddock/e2e-ready-reads -- <why>`. Better, wait with a ready helper from @decryp7/paddock-e2e (ui/projects/paddock-e2e/src/ready.ts) so the read needs no exception.';
const FIX_BLOCK = 'Use the one-line form, `// eslint-disable-next-line paddock/e2e-ready-reads -- <why>`: a block covers any number of reads.';
const FIX_WHY = 'Say why the read is immediate by design: `// eslint-disable-next-line paddock/e2e-ready-reads -- <why>` (at least three characters after the dashes).';
const FIX_LIST = "Wait with a ready helper from @decryp7/paddock-e2e (ui/projects/paddock-e2e/src/ready.ts) instead. An exception has to be listed in conformance/c139-allow.json (path, site `paddock/e2e-ready-reads`, reason, count), and the list only shrinks: a new entry needs the user's approval (ER-46).";

/** Whether `path` is a spec file this check reads. */
export const readable = (path) => E2E_TS.test(path) && !NOT_READ.test(path) && !IMPLEMENTATION.has(path);

/** [{ line, kind }] of every `eslint-disable` directive in `text` that names the rule or no rule; kind is unrestricted, block, no-reason or counted. */
export function sites(path, text) {
  if (!readable(path)) return [];
  const out = [];
  text.split('\n').forEach((raw, i) => {
    // A CRLF file: the carriage return is not part of the line (JavaScript's `.` stops at it, so both hosts drop it first).
    const line = raw.replace(/\r$/, '');
    const m = DIRECTIVE.exec(line);
    if (!m) return;
    const lineForm = m[1] !== undefined;
    const rest = m[2] ?? '';
    const at = rest.indexOf(' -- ');
    const [rules, why] = at < 0 ? [rest.trimEnd().replace(/\*\/$/, '').trimEnd(), ''] : [rest.slice(0, at), rest.slice(at + 4)];
    const names = rules.trimEnd().replace(/\*\/$/, '').split(',').map((s) => s.trim()).filter((s) => s !== '');
    let kind;
    if (names.length === 0) kind = 'unrestricted';
    else if (!names.includes(RULE_ID)) return;
    else if (!lineForm) kind = 'block';
    else if (why.trim().length >= 3) kind = 'counted';
    else kind = 'no-reason';
    out.push({ line: i + 1, kind });
  });
  return out;
}

/** The allow list: [{ path, site, reason, count }], or throws why it cannot be read (the shared parser). */
export const parseAllow = (text) => parseAllowList(text, ALLOW);
const SPEC = { id: 'C139', rule: 49, file: ALLOW, unseen: 'an e2e spec', restore: 'Restore conformance/c139-allow.json: {"allow": [{"path": "...", "site": "paddock/e2e-ready-reads", "reason": "...", "count": 1}]}.' };

export default {
  id: 'C139',
  scope: 'root',
  mode: 'report-only',
  allowList: SPEC,
  title: 'an e2e spec that disables paddock/e2e-ready-reads says why, and the exceptions only shrink',
  rule: 49,
  run(ctx) {
    const { list: allow, broken } = readAllowList(ctx, this.allowList);
    if (broken) return broken;
    const out = [];
    for (const f of ctx.files()) {
      if (!readable(f)) continue;
      const { text, finding } = allow.read(f);
      if (finding) {
        out.push(finding);
        continue;
      }
      for (const { line, kind } of sites(f, text)) {
        if (kind === 'unrestricted') out.push(warn('C139', 49, f, line, 'an eslint-disable in an e2e spec that names no rule switches off every rule on its line', FIX_RULES));
        else if (kind === 'block') out.push(warn('C139', 49, f, line, 'a block eslint-disable of paddock/e2e-ready-reads covers any number of reads', FIX_BLOCK));
        else if (kind === 'no-reason') out.push(warn('C139', 49, f, line, 'a disable of paddock/e2e-ready-reads without a description', FIX_WHY));
        else {
          const said = allow.admit(f, (site) => site === RULE_ID);
          if (said === 'allowed') continue;
          out.push(warn('C139', 49, f, line, said === 'beyond' ? 'a disable of paddock/e2e-ready-reads beyond the count of its allow entry' : 'a disable of paddock/e2e-ready-reads that the allow list does not name', FIX_LIST));
        }
      }
    }
    return [...out, ...allow.warnings()];
  },
};

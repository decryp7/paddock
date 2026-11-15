// C121: a test or a registry entry reads another part of the repository's declared data through its accessor, never by grepping or
// slicing the file's text (rule 79, ADR 0098 WP-2; the instance that started it: bddd2947c, a registry entry that ran `sed` over
// ci.yml for a line that train 7 had moved, while `ci_run::contract::conformance_args` already read it).
// Flagged, one line at a time:
// - in a registry (`docs/*.toml`): a line that names a workflow (`.gitea/workflows/`) and runs a text tool (grep, egrep, fgrep, sed,
//   awk, rg, perl);
// - in a test file (a `tests/` or `test/` folder, `*.test.*`, `tests.rs`, `*_test.rs`, `.test.sh`; Rust, JavaScript, TypeScript or
//   shell): such a line, or a line that names a workflow and applies a regular expression to it (`.match(`, `.matchAll(`,
//   `new RegExp(`, `Regex::new(`).
// Partly checked, said out loud: slicing another file's text by anchors and hard-coded paths into another crate (11c365fc4, 0308201b4)
// have no cheap low-noise lint; the rule, review and the train's test run cover them. A test of a check of workflows may read them:
// each such site goes in `conformance/c121-allow.json` with its path, a `site` (a piece of the flagged line), a reason and, for a
// line the file repeats, a `count`; the shared allow list (`util/allow-list.mjs`, also C124's) warns on a stale entry, reports a
// site beyond an entry's count at its own line, and never allows a whole file or folder.
// The shared reader owns the error paths: an allow file that cannot be read makes the check broken (one finding, nothing else), and a
// file the check cannot read is not measured (an error finding at that file).
// Not read: the check's own files (`IMPLEMENTATION`, as C120's): their invented lines are test data, not reads.
// Report-only (warnings) until its first green run on the real tree is recorded here (ER-41). Recorded: not yet. First count (2026-11-13): 10 findings, all the parity fixtures of C121 itself, now allowed with a reason.
// The Rust host (`tools/paddock-dev/src/check/c121.rs`) is the rule's own implementation; this is its port, and
// `tools/paddock-dev/tests/it/check.rs` (the `c121` tests) compares the two finding for finding.
import { warn } from '../finding.mjs';
import { parseAllowList, readAllowList } from '../util/allow-list.mjs';

export const ALLOW = 'conformance/c121-allow.json';
/** The check's own files, as C120's: its sources and tests name the patterns (their invented lines are test data), and its allow list. */
export const IMPLEMENTATION = new Set([
  'conformance/src/checks/C121-declared-data.mjs',
  'conformance/test/c121-declared-data.test.mjs',
  'tools/paddock-dev/src/check/c121.rs',
  'tools/paddock-dev/tests/it/check_c121.rs',
  ALLOW,
]);
const TOOL = /\b(?:grep|egrep|fgrep|sed|awk|rg|perl)\b/;
const WORKFLOW = /\.gitea\/workflows\/|(?:^|[\s"'`/])workflows\/[\w.-]+\.ya?ml/;
const REGEX_USE = /\.(?:match|matchAll)\(|new RegExp\(|Regex::new\(/;
const REGISTRY = /^docs\/[^/]+\.toml$/;
const TEST_PATH = /(?:^|\/)(?:tests?|__tests__)\/|\.(?:test|spec)\.[cm]?[jt]s$|(?:^|\/)tests\.rs$|_test\.rs$|\.test\.sh$/;
const TEST_EXT = /\.(?:rs|mjs|js|cjs|ts|sh)$/;
const NOT_TEST = /(?:^|\/)(?:fixtures|node_modules)\//;
const FIX =
  "Read the data through the one accessor that owns it (`./dev ci conformance-args`, `cargo metadata`, the register's parser, a constant the owner exports; rule 79). A test of a check of workflows may read them: list the site (its path, a piece of the line and a reason) in conformance/c121-allow.json.";

const BEYOND_FIX = `Read it through its accessor, or raise the entry's \`count\` in ${ALLOW} if this site is meant too.`;

/** 'registry', 'test' or null: how the check reads `path`. */
export function kindOf(path) {
  if (REGISTRY.test(path)) return 'registry';
  if (TEST_PATH.test(path) && TEST_EXT.test(path) && !NOT_TEST.test(path)) return 'test';
  return null;
}

/** [{ line, what, text }] of every line of `text` that reads a workflow's text with a text tool or, in a test, a regular expression. */
export function flagged(kind, text) {
  const out = [];
  text.split('\n').forEach((line, i) => {
    const t = line.trim();
    if (t.startsWith('#') || t.startsWith('//') || !WORKFLOW.test(line)) return;
    const tool = TOOL.exec(line);
    if (tool) out.push({ line: i + 1, what: `\`${tool[0]}\``, text: line });
    else if (kind === 'test' && REGEX_USE.test(line)) out.push({ line: i + 1, what: 'a regular expression', text: line });
  });
  return out;
}

/** The allow list: [{ path, site, reason, count }], or throws why it cannot be read (the shared parser, util/allow-list.mjs). */
export const parseAllow = (text) => parseAllowList(text, ALLOW);
const SPEC = { id: 'C121', rule: 79, file: ALLOW, unseen: 'a read of a workflow\'s text', restore: 'Restore conformance/c121-allow.json: {"allow": [{"path": "...", "site": "...", "reason": "..."}]}.' };

export default {
  id: 'C121',
  scope: 'root',
  mode: 'report-only',
  allowList: ALLOW,
  title: 'Declared data is read through its accessor, not by searching a workflow\'s text',
  rule: 79,
  run(ctx) {
    const { list: allow, broken } = readAllowList(ctx, SPEC);
    if (broken) return broken;
    const out = [];
    for (const f of ctx.files()) {
      const kind = IMPLEMENTATION.has(f) ? null : kindOf(f);
      if (!kind) continue;
      const { text, finding } = allow.read(f);
      if (finding) {
        out.push(finding);
        continue;
      }
      for (const { line, what, text: at } of flagged(kind, text)) {
        const said = allow.admit(f, (site) => at.includes(site));
        if (said === 'allowed') continue;
        out.push(
          said === 'beyond'
            ? warn('C121', 79, f, line, `reads a workflow file's text with ${what} beyond the count of its allow entry`, BEYOND_FIX)
            : warn('C121', 79, f, line, `reads a workflow file's text with ${what}: the data it needs has one accessor`, FIX),
        );
      }
    }
    return [...out, ...allow.warnings()];
  },
};

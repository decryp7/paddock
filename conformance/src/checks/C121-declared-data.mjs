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
// its path (a folder, ending in `/`, or a file) goes in `conformance/c121-allow.json` with a reason.
// Report-only (warnings) until its first green run on the real tree is recorded here (ER-41). Recorded: not yet. First count (2026-11-13): 10 findings, all the parity fixtures of C121 itself, now allowed with a reason.
// The Rust host (`tools/paddock-dev/src/check/c121.rs`) is the rule's own implementation; this is its port, and
// `tools/paddock-dev/tests/it/check.rs` (the `c121` tests) compares the two finding for finding.
import { unwaivable, warn } from '../finding.mjs';

export const ALLOW = 'conformance/c121-allow.json';
const TOOL = /\b(?:grep|egrep|fgrep|sed|awk|rg|perl)\b/;
const WORKFLOW = /\.gitea\/workflows\/|(?:^|[\s"'`/])workflows\/[\w.-]+\.ya?ml/;
const REGEX_USE = /\.(?:match|matchAll)\(|new RegExp\(|Regex::new\(/;
const REGISTRY = /^docs\/[^/]+\.toml$/;
const TEST_PATH = /(?:^|\/)(?:tests?|__tests__)\/|\.(?:test|spec)\.[cm]?[jt]s$|(?:^|\/)tests\.rs$|_test\.rs$|\.test\.sh$/;
const TEST_EXT = /\.(?:rs|mjs|js|cjs|ts|sh)$/;
const NOT_TEST = /(?:^|\/)(?:fixtures|node_modules)\//;
const FIX =
  "Read the data through the one accessor that owns it (`./dev ci conformance-args`, `cargo metadata`, the register's parser, a constant the owner exports; rule 79). A test of a check of workflows may read them: list its path with a reason in conformance/c121-allow.json.";

/** 'registry', 'test' or null: how the check reads `path`. */
export function kindOf(path) {
  if (REGISTRY.test(path)) return 'registry';
  if (TEST_PATH.test(path) && TEST_EXT.test(path) && !NOT_TEST.test(path)) return 'test';
  return null;
}

/** [{ line, what }] of every line of `text` that reads a workflow's text with a text tool or, in a test, a regular expression. */
export function flagged(kind, text) {
  const out = [];
  text.split('\n').forEach((line, i) => {
    const t = line.trim();
    if (t.startsWith('#') || t.startsWith('//') || !WORKFLOW.test(line)) return;
    const tool = TOOL.exec(line);
    if (tool) out.push({ line: i + 1, what: `\`${tool[0]}\`` });
    else if (kind === 'test' && REGEX_USE.test(line)) out.push({ line: i + 1, what: 'a regular expression' });
  });
  return out;
}

/** The allow list: [{ path, reason }], or throws why it cannot be read. */
export function parseAllow(text) {
  let v;
  try {
    v = JSON.parse(text);
  } catch (e) {
    throw new Error(`${ALLOW} is not JSON: ${e.message}`);
  }
  if (!Array.isArray(v?.allow)) throw new Error(`${ALLOW} has no \`allow\` array`);
  return v.allow.map((e) => {
    const path = typeof e?.path === 'string' ? e.path : '';
    const reason = typeof e?.reason === 'string' ? e.reason.trim() : '';
    if (!path || [...reason].length < 8) throw new Error(`${ALLOW}: every entry needs a path and a reason of at least 8 characters`);
    return { path, reason };
  });
}

const isAllowed = (allow, path) => allow.some((a) => (a.path.endsWith('/') ? path.startsWith(a.path) : path === a.path));

export default {
  id: 'C121',
  scope: 'root',
  mode: 'report-only',
  title: 'Declared data is read through its accessor, not by searching a workflow\'s text',
  rule: 79,
  run(ctx) {
    const raw = ctx.read(ALLOW);
    let allow = [];
    try {
      if (raw === null || raw === undefined) throw new Error(`${ALLOW} cannot be read`);
      allow = parseAllow(raw);
    } catch (e) {
      return [unwaivable(warn('C121', 79, ALLOW, 1, `the check cannot look: ${e.message}`, 'Restore conformance/c121-allow.json: {"allow": [{"path": "...", "reason": "..."}]}.'), 'check-broken')];
    }
    const out = [];
    for (const f of ctx.files()) {
      const kind = kindOf(f);
      if (!kind || isAllowed(allow, f)) continue;
      const text = ctx.read(f);
      if (text === null || text === undefined) continue;
      for (const { line, what } of flagged(kind, text)) {
        out.push(warn('C121', 79, f, line, `reads a workflow file's text with ${what}: the data it needs has one accessor`, FIX));
      }
    }
    return out;
  },
};

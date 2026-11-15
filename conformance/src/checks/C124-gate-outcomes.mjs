// C124: a gate's result is built through the shared outcome, never by hand (ADR 0089 amendment 2026-11-13, WP-T6; ER-21: the class
// "a gate gives its verdict from an indirect signal" recurred at rows 2 to 7 of the amendment's table).
// Flagged, one site at a time, in the gate and check tools' Rust (`tools/paddock-dev/src/`, `crates/paddock-bench/src/`) outside test code (a `tests/`
// folder, `tests.rs`, `*_test.rs`, and everything after a file's first `#[cfg(test)]`):
// - the `NotMeasured`, `Clean`, `Pass`, `Passed`, `Success`, `Current` or `Green` variants of a local enum that is a result type, one
//   with a `NotMeasured` variant or two or more result words (those, and `Red`, `Stale`, `Fail`, `Failed`, `Findings`, `Dirty`,
//   `Outdated`, `Broken`, `Error`, `Mismatch`, `Missing`, `Skipped`):
//   a second result type (use `paddock_cli::Outcome`); one such word among others names a phase or a command (`PhaseId::Clean`);
// - a JSON object literal with a `"status"`, `"verdict"` or `"outcome"` key: a result a consumer would read without evidence
//   (use `Outcome::json()`, read it with `Outcome::from_wire`);
// - `fn fails(&self) -> bool` or `fn passed(&self) -> bool`: the bool conversion the shared type forbids;
// - a call of `Evidence::ci_run(` or `Evidence::adopt(` outside the modules that own them (`ci_runs/verdict.rs`, `pre_push/known.rs`
//   until WP-T4 moves the CI reader's evidence to `ci_runs::verdict`);
// - `measure(.., || Ok(Vec::new()))` (also `vec![]`, `Vec::default()`, `Default::default()`): a measurement that always finds nothing
//   reads as a pass; return the not-measured outcome when the check cannot look, or build the findings from what it saw.
// In JavaScript, outside `conformance/src/outcome.mjs`: an object literal `status: 'clean'` (a pass made by hand).
// The shared reader owns the error paths: an allow file that cannot be read makes the check broken (one finding, nothing else), and a
// file the check cannot read is not measured (an error finding at that file).
// Pre-existing sites are listed one by one in `conformance/c124-allow.json`: a path, a `site` (a piece of the flagged line, so an edit
// that moves the line keeps the entry) and a reason, read and counted by the shared allow list (`util/allow-list.mjs`, also C121's): a
// whole file or folder is never allowed, an entry that matches no site is stale (a warning) and a site beyond an entry's
// `count` is reported at its own line, as PHPStan's reportUnmatchedIgnoredErrors and baseline `count` do. A shrink-only seed, expected to empty
// out as each gate migrates. Report-only (warnings) until its first green run on the real tree is recorded here (ER-41). Recorded: not yet.
import { warn } from '../finding.mjs';
import { parseAllowList, readAllowList } from '../util/allow-list.mjs';

export const ALLOW = 'conformance/c124-allow.json';
const RUST = /^(?:tools\/paddock-dev\/src|crates\/paddock-bench\/src)\/.*\.rs$/;
const OWNER = /^crates\/paddock-cli\//;
const TEST_PATH = /(?:^|\/)(?:tests?|__tests__)\/|(?:^|\/)tests\.rs$|_test\.rs$/;
const VARIANT = /^\s*(NotMeasured|Clean|Pass|Passed|Success|Current|Green)\b/;
// The words of a result: an enum is a second result type when it has a `NotMeasured` variant or two or more of these (`Earlier { Clean,
// Failed }`), never for one word among others (`PhaseId { Resolve, Clean, Fetch }`, `DiskCommand { Clean(..) }`: a phase or a command).
const RESULT_WORD = /^\s*(NotMeasured|Clean|Pass|Passed|Success|Current|Green|Red|Stale|Fail|Failed|Findings|Dirty|Outdated|Broken|Error|Mismatch|Missing|Skipped)\b/;
const ENUM = /^\s*(?:pub(?:\([^)]*\))?\s+)?enum\s+\w+/;
const JSON_KEY = /"(?:status|verdict|outcome)"\s*:/;
const BOOL_FN = /\bfn\s+(?:fails|passed)\s*\(\s*&self\s*\)\s*->\s*bool\b/;
const CALL = /\b(?:Evidence|Proof)::(?:ci_run|adopt)\s*\(/;
const CALL_OWNERS = new Set(['tools/paddock-dev/src/ci_runs/verdict.rs', 'tools/paddock-dev/src/pre_push/known.rs']);
const JS_FILE = /^(?:conformance\/(?:src|bin)|scripts|tools)\/.*\.mjs$/;
const JS_PASS = /\bstatus\s*:\s*['"]clean['"]/;
// The closure of a measurement that is always empty: the last argument of a `measure(` call is `|| Ok(<empty>)`, with or without braces.
const EMPTY_TAIL = /\|\|\s*(?:\{\s*)?Ok\(\s*(?:Vec::new\(\)|vec!\s*\[\s*\]|Vec::default\(\)|Default::default\(\))\s*\)\s*;?\s*\}?\s*,?\s*$/;
const MEASURE_CALL = /\bmeasure\s*\(/g;
const FIX = 'Build the result with `paddock_cli::measure` (or `Evidence::adopt`) and print it with `Outcome::json()`; read another gate\'s result with `Outcome::from_wire`, never its exit code (ADR 0089 amendment 2026-11-13).';

/** [{ line, what, text }] of every site in the Rust text `text` of `path`. */
export function flaggedRust(path, text) {
  if (OWNER.test(path) || TEST_PATH.test(path)) return [];
  const out = [];
  const lines = text.split('\n');
  const end = lines.findIndex((l) => /^\s*#\[cfg\(test\)\]/.test(l));
  const body = end < 0 ? lines : lines.slice(0, end);
  let depth = -1; // brace depth inside an enum, -1 outside
  let header = ''; // the enum's own line: its name is part of a variant's site, so `NotMeasured,` of two enums are two sites
  let candidates = []; // the enum's flaggable variants, kept until the enum closes and shows whether it is a result type
  let words = new Set(); // the enum's result words
  body.forEach((line, i) => {
    const t = line.trim();
    if (t.startsWith('//')) return;
    if (depth < 0 && ENUM.test(line) && line.includes('{')) {
      depth = 0;
      header = t;
      candidates = [];
      words = new Set();
    }
    if (depth >= 0) {
      if (depth === 1) {
        const v = VARIANT.exec(line);
        if (v) candidates.push({ line: i + 1, text: `${header} ${t}`, what: `an enum variant \`${v[1]}\` (a second result type)` });
        const w = RESULT_WORD.exec(line);
        if (w) words.add(w[1]);
      }
      for (const c of line) depth += c === '{' ? 1 : c === '}' ? -1 : 0;
      if (depth <= 0) {
        depth = -1;
        if (words.has('NotMeasured') || words.size >= 2) out.push(...candidates);
      }
    }
    if (JSON_KEY.test(line) && /json!|serde_json/.test(text)) out.push({ line: i + 1, text: line, what: 'a JSON result object with a status, verdict or outcome key' });
    if (BOOL_FN.test(line)) out.push({ line: i + 1, text: line, what: 'a `fails`/`passed` bool on a result type' });
    if (CALL.test(line) && !CALL_OWNERS.has(path)) out.push({ line: i + 1, text: line, what: 'a call of `Evidence::ci_run`/`adopt` outside its owner' });
  });
  // Comment lines are blanked (line numbers stay), then each `measure(` call is cut at its balanced closing parenthesis.
  const code = body.map((l) => (l.trim().startsWith('//') ? '' : l)).join('\n');
  for (const m of code.matchAll(MEASURE_CALL)) {
    let depth = 1;
    let k = m.index + m[0].length;
    for (; k < code.length && depth > 0; k++) depth += code[k] === '(' ? 1 : code[k] === ')' ? -1 : 0;
    const call = code.slice(m.index + m[0].length, k - 1);
    if (depth !== 0 || !EMPTY_TAIL.test(call)) continue;
    const line = code.slice(0, m.index).split('\n').length;
    out.push({ line, text: body[line - 1], what: 'a `measure` whose closure always returns an empty list (an empty measurement reads as a pass)' });
  }
  // An enum's variants are known only when it closes: back in line order (stable, as the Rust host sorts).
  return out.sort((a, b) => a.line - b.line);
}

/** [{ line, what, text }] of every hand-made pass in the JavaScript text of `path`. */
export function flaggedJs(path, text) {
  if (path === 'conformance/src/outcome.mjs' || path === 'conformance/src/checks/C124-gate-outcomes.mjs' || TEST_PATH.test(path) || /\.test\.mjs$/.test(path)) return [];
  const out = [];
  text.split('\n').forEach((line, i) => {
    if (line.trim().startsWith('//')) return;
    if (JS_PASS.test(line)) out.push({ line: i + 1, text: line, what: "an object literal `status: 'clean'` (a pass made by hand)" });
  });
  return out;
}

/** The allow list: [{ path, site, reason, count }], or throws why it cannot be read (the shared parser, util/allow-list.mjs). */
export const parseAllow = (text) => parseAllowList(text, ALLOW);
const SPEC = { id: 'C124', rule: 21, file: ALLOW, unseen: 'a gate result made by hand', restore: 'Restore conformance/c124-allow.json: {"allow": [{"path": "...", "site": "...", "reason": "..."}]}.' };

export default {
  id: 'C124',
  scope: 'root',
  mode: 'report-only',
  allowList: ALLOW,
  title: "A gate's result is built through the shared outcome, never by hand",
  rule: 21,
  run(ctx) {
    const { list: allow, broken } = readAllowList(ctx, SPEC);
    if (broken) return broken;
    const out = [];
    for (const f of ctx.files()) {
      const rust = RUST.test(f);
      if (!rust && !JS_FILE.test(f)) continue;
      const { text, finding } = allow.read(f);
      if (finding) {
        out.push(finding);
        continue;
      }
      for (const { line, what, text: at } of rust ? flaggedRust(f, text) : flaggedJs(f, text)) {
        const said = allow.admit(f, (site) => at.includes(site));
        if (said === 'allowed') continue;
        out.push(
          said === 'beyond'
            ? warn('C124', 21, f, line, `builds a gate result by hand beyond the count of its allow entry: ${what}`, `Migrate it, or raise the entry's \`count\` in ${ALLOW} if this site is meant too.`)
            : warn('C124', 21, f, line, `builds a gate result by hand: ${what}`, FIX),
        );
      }
    }
    return [...out, ...allow.warnings()];
  },
};

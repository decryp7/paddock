// C124: a gate's result is built through the shared outcome, never by hand (ADR 0089 amendment 2026-11-13, WP-T6; ER-21: the class
// "a gate gives its verdict from an indirect signal" recurred at rows 2 to 7 of the amendment's table).
// Flagged, one site at a time, in the gate and check tools' Rust (`tools/paddock-dev/src/`, `crates/paddock-bench/src/`) outside test code (a `tests/`
// folder, `tests.rs`, `*_test.rs`, and everything after a file's first `#[cfg(test)]`):
// - a local enum with a `NotMeasured`, `Clean`, `Pass`, `Current` or `Green` variant: a second result type (use `paddock_cli::Outcome`);
// - a JSON object literal with a `"status"`, `"verdict"` or `"outcome"` key: a result a consumer would read without evidence
//   (use `Outcome::json()`, read it with `Outcome::from_wire`);
// - `fn fails(&self) -> bool` or `fn passed(&self) -> bool`: the bool conversion the shared type forbids;
// - a call of `Evidence::ci_run(` or `Evidence::adopt(` outside the modules that own them (`ci_runs/verdict.rs`, `pre_push/known.rs`
//   until WP-T4 moves the CI reader's evidence to `ci_runs::verdict`).
// In JavaScript, outside `conformance/src/outcome.mjs`: an object literal `status: 'clean'` (a pass made by hand).
// Pre-existing sites are listed by path in `conformance/c124-allow.json` with a reason: a shrink-only seed, expected to empty out as
// each gate migrates. Report-only (warnings) until its first green run on the real tree is recorded here (ER-41). Recorded: not yet.
import { unwaivable, warn } from '../finding.mjs';

export const ALLOW = 'conformance/c124-allow.json';
const RUST = /^(?:tools\/paddock-dev\/src|crates\/paddock-bench\/src)\/.*\.rs$/;
const OWNER = /^crates\/paddock-cli\//;
const TEST_PATH = /(?:^|\/)(?:tests?|__tests__)\/|(?:^|\/)tests\.rs$|_test\.rs$/;
const VARIANT = /^\s*(NotMeasured|Clean|Pass|Current|Green)\b/;
const ENUM = /^\s*(?:pub(?:\([^)]*\))?\s+)?enum\s+\w+/;
const JSON_KEY = /"(?:status|verdict|outcome)"\s*:/;
const BOOL_FN = /\bfn\s+(?:fails|passed)\s*\(\s*&self\s*\)\s*->\s*bool\b/;
const CALL = /\b(?:Evidence|Proof)::(?:ci_run|adopt)\s*\(/;
const CALL_OWNERS = new Set(['tools/paddock-dev/src/ci_runs/verdict.rs', 'tools/paddock-dev/src/pre_push/known.rs']);
const JS_FILE = /^(?:conformance\/(?:src|bin)|scripts|tools)\/.*\.mjs$/;
const JS_PASS = /\bstatus\s*:\s*['"]clean['"]/;
const FIX = 'Build the result with `paddock_cli::measure` (or `Evidence::adopt`) and print it with `Outcome::json()`; read another gate\'s result with `Outcome::from_wire`, never its exit code (ADR 0089 amendment 2026-11-13).';

/** [{ line, what }] of every site in the Rust text `text` of `path`. */
export function flaggedRust(path, text) {
  if (OWNER.test(path) || TEST_PATH.test(path)) return [];
  const out = [];
  const lines = text.split('\n');
  const end = lines.findIndex((l) => /^\s*#\[cfg\(test\)\]/.test(l));
  const body = end < 0 ? lines : lines.slice(0, end);
  let depth = -1; // brace depth inside an enum, -1 outside
  body.forEach((line, i) => {
    const t = line.trim();
    if (t.startsWith('//')) return;
    if (depth < 0 && ENUM.test(line) && line.includes('{')) depth = 0;
    if (depth >= 0) {
      if (depth === 1) {
        const v = VARIANT.exec(line);
        if (v) out.push({ line: i + 1, what: `an enum variant \`${v[1]}\` (a second result type)` });
      }
      for (const c of line) depth += c === '{' ? 1 : c === '}' ? -1 : 0;
      if (depth <= 0) depth = -1;
    }
    if (JSON_KEY.test(line) && /json!|serde_json/.test(text)) out.push({ line: i + 1, what: 'a JSON result object with a status, verdict or outcome key' });
    if (BOOL_FN.test(line)) out.push({ line: i + 1, what: 'a `fails`/`passed` bool on a result type' });
    if (CALL.test(line) && !CALL_OWNERS.has(path)) out.push({ line: i + 1, what: 'a call of `Evidence::ci_run`/`adopt` outside its owner' });
  });
  return out;
}

/** [{ line, what }] of every hand-made pass in the JavaScript text of `path`. */
export function flaggedJs(path, text) {
  if (path === 'conformance/src/outcome.mjs' || path === 'conformance/src/checks/C124-gate-outcomes.mjs' || TEST_PATH.test(path) || /\.test\.mjs$/.test(path)) return [];
  const out = [];
  text.split('\n').forEach((line, i) => {
    if (line.trim().startsWith('//')) return;
    if (JS_PASS.test(line)) out.push({ line: i + 1, what: "an object literal `status: 'clean'` (a pass made by hand)" });
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
  id: 'C124',
  scope: 'root',
  mode: 'report-only',
  title: "A gate's result is built through the shared outcome, never by hand",
  rule: 21,
  run(ctx) {
    const raw = ctx.read(ALLOW);
    let allow = [];
    try {
      if (raw === null || raw === undefined) throw new Error(`${ALLOW} cannot be read`);
      allow = parseAllow(raw);
    } catch (e) {
      return [unwaivable(warn('C124', 21, ALLOW, 1, `the check cannot look: ${e.message}`, 'Restore conformance/c124-allow.json: {"allow": [{"path": "...", "reason": "..."}]}.'), 'check-broken')];
    }
    const out = [];
    for (const f of ctx.files()) {
      const rust = RUST.test(f);
      if ((!rust && !JS_FILE.test(f)) || isAllowed(allow, f)) continue;
      const text = ctx.read(f);
      if (text === null || text === undefined) continue;
      for (const { line, what } of rust ? flaggedRust(f, text) : flaggedJs(f, text)) {
        out.push(warn('C124', 21, f, line, `builds a gate result by hand: ${what}`, FIX));
      }
    }
    return out;
  },
};

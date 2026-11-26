// C171: a Rust test builds an absolute path from parts under a temporary folder (`paddock_testkit::path::under`, `abs`), never from a
// Unix-style literal such as `"/srv/dt"`. On Windows `/srv/dt` is not absolute (no drive), so a test that passes it to a validator
// (`Path::is_absolute`, `dev_targets_of`, ...) fails there only: two Windows-only CI failures on 2026-11-25 (`join("wt/a/crates")` read
// back as text, and "/dev-targets-x" is not absolute). Compare paths with `paddock_testkit::path::same_path`, never as text.
//
// Flagged, in Rust test code (the files under `tests/`, `tests.rs`, `*_test(s).rs`, and the `#[cfg(test)]` items of other files;
// comments and string insides are masked, so only real code is read): a string literal that starts with `/` and a name, given to
// `Path::new(..)`, `PathBuf::from(..)` or a known validator (VALIDATORS: `dev_targets_of`). Not flagged: an item behind `#[cfg(unix)]`
// or `#[cfg(not(windows))]`, a file that starts with `#![cfg(unix)]` (a `/dev/null` case is Unix by nature), and a site with
// `// conformance: abs-path — <why>` on its line or the line above.
// Split (user decision 2026-11-26, after a third Windows-only failure of this class, run 9116, `"/dev-targets-y"` in a test of
// pre_push/exec.rs): a site on a line the change ADDS (`git diff -U0` against the merge base with the gate's base ref, the working tree
// included) is an ERROR (the check is a gate; ER-41 report-only phase skipped for new lines by that decision, recorded here because a gate has no
// docs/gates.toml register entry), so no new Unix literal lands; every existing site stays a WARNING until the 2026-12-09 review in
// docs/gates.toml sorts them (ER-41). When the base cannot be read the added lines are unknown: a warning outside CI, a not-measured
// error in CI (ER-33, ADR 0089: a blocking check that cannot see its base does not pass).
import { execFileSync } from 'node:child_process';
import { error, warn } from '../finding.mjs';
import { targetRef, unmeasuredFinding } from '../util/baseline.mjs';
import { maskRust } from '../util/mask-rust.mjs';
import { onlyTestItems, withoutTestItems } from '../util/rust-test-items.mjs';

/** Functions that validate or derive a path and are given path text in tests; add a name here when a new validator needs the same care. */
const VALIDATORS = ['dev_targets_of'];

const WAIVER = /conformance:\s*abs-path\s*[—–-]+\s*\S.{2,}/;
const SKIP_PATH = /(?:^|\/)(?:target|vendor|node_modules)\/|(?:^|\/)fixtures?\/|^apps\/debrief\/docs\/research\//;
const TEST_FILE = /(?:^|\/)tests\/.+\.rs$|(?:^|\/)tests\.rs$|_tests?\.rs$/;
const CALL = new RegExp(`(?<![\\w])(?:(?:std::path::)?(?:Path::new|PathBuf::from)|(?:${VALIDATORS.join('|')}))\\s*\\(`, 'g');
const UNIX_LITERAL = /"(\/[A-Za-z0-9_.~-][^"\n]*)"/g;
const FIX = 'Use paddock-testkit::path (rule 42, share first). Build the path from parts under a temporary folder with `paddock_testkit::path::under` (or `paddock_testkit::path::abs` where no folder is needed) and compare with `paddock_testkit::path::same_path`, never as text. A Unix-only case sits behind `#[cfg(unix)]` or carries `// conformance: abs-path — <why>`.';

/** Whether a `cfg(..)` predicate (its inner text) names Unix only. The string insides are masked, so only `unix` and `not(windows)`. */
const unixOnly = (p) => /^\s*unix\s*$/.test(p) || /^\s*not\s*\(\s*windows\s*\)\s*$/.test(p);

/**
 * Whether a literal that follows `before` (the call's argument text up to it) is itself an argument: `("/a"`, `(x, "/a"` and `(Some("/a")`
 * are; `(abs("/a")` is not, because a helper turned it into a path of this OS (a wrapper is the fix this check asks for).
 */
function isDirectArgument(before) {
  let depth = 0;
  const opened = [];
  for (const m of before.matchAll(/(\w*)\s*\(|\)/g)) {
    if (m[0] === ')') {
      opened.pop();
      continue;
    }
    opened.push(m[1]);
  }
  depth = opened.filter((w) => w !== 'Some').length;
  return depth === 0 && /(?:^|[,(])\s*&?\s*$/.test(before);
}

/** The sites of one file: [{ line }]. Empty for a file that is not Rust test code. */
export function absPathSites(text, file) {
  if (!file.endsWith('.rs') || SKIP_PATH.test(file)) return [];
  const masked = maskRust(text);
  if (/^\s*#!\[\s*cfg\s*\(\s*unix\s*\)\s*\]/m.test(masked)) return [];
  const region = withoutTestItems(TEST_FILE.test(file) ? masked : onlyTestItems(masked), unixOnly);
  const lines = text.split('\n');
  const out = [];
  for (const m of region.matchAll(CALL)) {
    let depth = 1;
    let end = m.index + m[0].length;
    for (; end < region.length && depth > 0; end++) depth += region[end] === '(' ? 1 : region[end] === ')' ? -1 : 0;
    const from = m.index + m[0].length;
    for (const lit of text.slice(from, end).matchAll(UNIX_LITERAL)) {
      const at = from + lit.index;
      if (region[at] !== '"') continue; // masked away: a comment or the inside of another literal
      if (!isDirectArgument(region.slice(from, at))) continue;
      const line = text.slice(0, at).split('\n').length;
      if (WAIVER.test(lines[line - 1] ?? '') || WAIVER.test(lines[line - 2] ?? '')) continue;
      if (!out.some((o) => o.line === line)) out.push({ line });
    }
  }
  return out;
}

/** The lines a unified diff (`git diff -U0`) adds: a Map from file to the Set of its new line numbers. */
export function addedLines(diff) {
  const out = new Map();
  let file = null;
  for (const l of diff.split('\n')) {
    if (l.startsWith('+++ ')) {
      file = l === '+++ /dev/null' ? null : l.replace(/^\+\+\+ b\//, '');
      if (file !== null && !out.has(file)) out.set(file, new Set());
      continue;
    }
    const h = /^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@/.exec(l);
    if (h && file !== null) {
      const start = Number(h[1]);
      const count = h[2] === undefined ? 1 : Number(h[2]);
      for (let n = start; n < start + count; n++) out.get(file).add(n);
    }
  }
  return out;
}

/** The lines added since the merge base with the run's base ref, or `{ reason }` when git cannot say. */
function addedSince(ctx) {
  const git = ctx.options?.git ?? ((args) => execFileSync('git', args, { cwd: ctx.root, encoding: 'utf8', maxBuffer: 64 << 20, stdio: ['ignore', 'pipe', 'pipe'] }));
  try {
    const head = git(['rev-parse', 'HEAD']).trim();
    let base = git(['merge-base', 'HEAD', targetRef(ctx.options)]).trim();
    if (base === head) base = git(['rev-parse', 'HEAD^1']).trim();
    return addedLines(git(['diff', '-U0', '--no-renames', '--no-color', base, '--', '*.rs']));
  } catch (e) {
    return { reason: `no merge base or diff (${String(e.message ?? e).split('\n')[0].slice(0, 120)})` };
  }
}

export default {
  id: 'C171',
  scope: 'root',
  mode: 'gate',
  title: 'Rust tests build absolute paths with paddock_testkit::path, never from a Unix-style literal',
  rule: 42,
  run(ctx) {
    const sites = [];
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs')) continue;
      const text = ctx.read(f);
      if (!text || !text.includes('"/')) continue;
      for (const site of absPathSites(text, f)) sites.push({ file: f, line: site.line });
    }
    if (sites.length === 0) return [];
    const added = addedSince(ctx);
    const out = [];
    if (added.reason) out.push(unmeasuredFinding(ctx, { id: 'C171', rule: 42, file: 'conformance/src/checks/C171-abs-path-literals.mjs', what: 'the added-line block', g: { kind: 'no-base', reason: added.reason } }));
    for (const { file, line } of sites) {
      const isNew = !added.reason && added.get(file)?.has(line);
      const message = 'passes a Unix-style absolute path literal to a path constructor or validator: it is not absolute on Windows (the 2026-11-25 Windows-only CI failures)';
      out.push(isNew ? error('C171', 42, file, line, `${message}; this line is new, so it blocks`, FIX) : warn('C171', 42, file, line, message, FIX));
    }
    return out;
  },
};

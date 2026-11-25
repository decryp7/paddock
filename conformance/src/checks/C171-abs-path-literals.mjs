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
// ER-41: report-only (warnings) until its first green run on the real tree, recorded in docs/gates.toml; review date 2026-12-26 there.
import { warn } from '../finding.mjs';
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

export default {
  id: 'C171',
  scope: 'root',
  mode: 'report-only',
  title: 'Rust tests build absolute paths with paddock_testkit::path, never from a Unix-style literal',
  rule: 42,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs')) continue;
      const text = ctx.read(f);
      if (!text || !text.includes('"/')) continue;
      for (const s of absPathSites(text, f)) {
        out.push(warn('C171', 42, f, s.line, 'passes a Unix-style absolute path literal to a path constructor or validator: it is not absolute on Windows (the 2026-11-25 Windows-only CI failures)', FIX));
      }
    }
    return out;
  },
};

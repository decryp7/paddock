// C121: tests compare paths as paths (rule 40, ADR 0099; the class: runs 7943, 8027, 8034, 8050 each failed only on the Windows
// runner). In Rust test code (files under `tests/`, `tests.rs`, `*_tests.rs`, and the `#[cfg(test)] mod` of any other file) it reads,
// with comments and string insides masked as C53 does:
// - rule A: a `format!` that joins a `.display()` argument with a literal `/` or `\` next to the placeholder
//   (`format!("{}/x", p.display())`): Windows prints `C:\a/x`. Use `Path::join`, `paddock_testkit::path::under` or `::text`;
// - rule B: a hand separator fold, `.replace('\\', "/")` (also with strings): use `paddock_testkit::path::text` or `same_path`;
// - rule C (warning only, a heuristic): `assert_eq!`/`assert_ne!` with an operand that is path text (`.display().to_string()`,
//   `.to_str().unwrap()`, `.to_string_lossy()`, none of a `file_name()`): compare Paths, or use `assert_same_path!`.
// A site that is not a path comparison carries `// conformance: path-text — <why>` on the line or the line above.
// ER-41: report-only until the first green run on the real tree; A and B are warnings now (flip SEVERITY and the mode to gate).
// The Rust host has the same check (`tools/paddock-dev/src/check/c121.rs`); `tests/it/check_c121.rs` compares them.
import { warn } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

export const SEVERITY = warn;
const WAIVER = /conformance:\s*path-text\s*[—–-]+\s*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*path-text/;
const SKIP_PATH = /(?:^|\/)(?:node_modules|dist|target)\/|(?:^|\/)tests\/fixtures\/|^apps\/[^/]+\/vendor\/|^conformance\/(?:test|fixtures)\//;
const TEST_FILE = /(?:^|\/)tests\/|(?:^|\/)tests\.rs$|_tests\.rs$/;
const CFG_TEST_MOD = /#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{/;
const FORMAT_LITERAL = /\}(?:\/|\\\\)|(?:\/|\\\\)\{/;
const FOLD = /\.replace\(\s*(?:'\\\\'|"\\\\"|r#*"\\"#*)\s*,\s*(?:"\/"|'\/')\s*\)/g;
const PATH_TEXT = /\.display\(\)\.to_string\(\)|\.to_str\(\)\.unwrap\(\)|\.to_string_lossy\(\)/;

/** The index just past the `)` that closes the `(` at `open` in `masked`, or -1. */
function closeOf(masked, open) {
  let depth = 0;
  for (let i = open; i < masked.length; i++) {
    if (masked[i] === '(') depth++;
    else if (masked[i] === ')' && --depth === 0) return i + 1;
  }
  return -1;
}

/** The first string literal of the call `text[open..end)` as written in the source (escapes unread), or ''. */
function firstLiteral(text, masked, open, end) {
  const a = masked.indexOf('"', open);
  if (a < 0 || a >= end) return '';
  const b = masked.indexOf('"', a + 1);
  return b < 0 ? '' : text.slice(a + 1, b);
}

/** The path-text sites of one Rust file: [{ line, rule: 'A' | 'B' | 'C', what, waived: 'reason' | 'no-reason' | undefined }]. */
export function pathTextSites(text, file) {
  if (SKIP_PATH.test(file)) return [];
  const masked = maskRust(text);
  let from = 0;
  if (!TEST_FILE.test(file)) {
    const m = CFG_TEST_MOD.exec(masked);
    if (!m) return [];
    from = m.index;
  }
  const lines = text.split('\n');
  const lineAt = (i) => text.slice(0, i).split('\n').length;
  const out = [];
  const push = (index, rule, what) => {
    const line = lineAt(index);
    const near = `${lines[line - 2] ?? ''}\n${lines[line - 1] ?? ''}`;
    const w = WAIVER.exec(near);
    out.push({ index, line, rule, what, waived: w ? 'reason' : BARE_WAIVER.test(near) ? 'no-reason' : undefined });
  };
  for (const m of masked.matchAll(/\bformat!\s*\(/g)) {
    if (m.index < from) continue;
    const open = m.index + m[0].length - 1;
    const end = closeOf(masked, open);
    if (end < 0) continue;
    if (!masked.slice(open, end).includes('.display()')) continue;
    if (FORMAT_LITERAL.test(firstLiteral(text, masked, open, end))) {
      push(m.index, 'A', 'joins a `.display()` argument with a literal `/` or `\\`: Windows spells the result `C:\\a/x`');
    }
  }
  for (const m of text.matchAll(FOLD)) {
    if (m.index < from || masked.slice(m.index, m.index + 9) !== '.replace(') continue;
    push(m.index, 'B', 'folds path separators by hand with `.replace`');
  }
  for (const m of masked.matchAll(/\bassert_(?:eq|ne)!\s*\(/g)) {
    if (m.index < from) continue;
    const open = m.index + m[0].length - 1;
    const end = closeOf(masked, open);
    if (end < 0) continue;
    const args = masked.slice(open, end);
    if (PATH_TEXT.test(args) && !args.includes('file_name()')) push(m.index, 'C', 'compares path text, not paths');
  }
  return out.sort((a, b) => a.index - b.index || a.rule.localeCompare(b.rule));
}

const FIX = {
  A: 'Build the path with `Path::join` or `paddock_testkit::path::under`, or the expected text with `paddock_testkit::path::text`.',
  B: 'Use `paddock_testkit::path::text` (the tools\' spelling) or compare with `same_path` / `assert_same_path!`.',
  C: 'Compare `Path`s, or use `paddock_testkit::assert_same_path!`; keep text only for what a program printed (`path::text`).',
};

export default {
  id: 'C121',
  scope: 'root',
  mode: 'report-only',
  title: 'Tests compare paths as paths',
  rule: 40,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs')) continue;
      const text = ctx.read(f);
      if (!text || !/format!|\.replace\(|assert_(?:eq|ne)!/.test(text)) continue;
      for (const s of pathTextSites(text, f)) {
        if (s.waived === 'reason') continue;
        const bare = s.waived === 'no-reason' ? ' (the waiver comment has no reason)' : '';
        const make = s.rule === 'C' ? warn : SEVERITY;
        out.push(make('C121', 40, f, s.line, `${s.what}${bare} (rule ${s.rule}: the same place is spelled differently on Windows)`, `${FIX[s.rule]} A site that is not a path comparison carries \`// conformance: path-text — <why>\`.`));
      }
    }
    return out;
  },
};

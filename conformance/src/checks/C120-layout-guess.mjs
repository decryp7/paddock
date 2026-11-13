// C120 (report-only until its first green run, ER-41): nothing guesses cargo's build-folder layout (ER-21). "Two folders up from
// `current_exe()` or `OUT_DIR`" is the profile folder in one cargo layout and a wrong folder in the other: it broke three times (Pitcrew's
// supervision tests, CI run 8027; Debrief's plugins tests, run 8028; paddock-build). The one reader of both layouts is
// `paddock_env::cargo_layout` (`paddock_testkit::cargo_layout` for tests). In Rust and shell under `crates/`, `tools/`, `apps/` and
// `scripts/`, outside those two files, this flags
// - `current_exe()` or `OUT_DIR` followed, in the same statement, by two or more `.parent()` calls, `.ancestors().nth(n)` with n >= 2, or
//   two `..` hops (`"../.."`, two `join("..")`); in shell, `OUT_DIR` with `../..` or two `dirname` on the line, in either order;
// - a literal `/deps/` or `\deps\` path segment, a path that ends in `/deps` (`target/debug/deps"`), and `.join("deps")` / `.push("deps")`.
// Not read: lines that start with a comment (`//`, `#`), a trailing comment (white space then `//`, or `#` in shell), and a `deps` in a URL
// (`https?://`) or a `node_modules/` path. Limits: a statement split by `;` (`let e = current_exe()?; e.parent()...`) is not
// followed; a block comment or a trailing comment is read as code. A justified site carries `// conformance: layout-guess — <why>`
// (`# conformance: ...` in shell) on the line or the line above. Rust host: tools/paddock-dev/src/check/c120.rs (same findings, line for line).
import { warn } from '../finding.mjs';

/** The files that read both layouts, and the check's own host port (its docs and tests name the patterns). */
export const IMPLEMENTATION = new Set(['crates/paddock-env/src/cargo_layout.rs', 'crates/paddock-testkit/src/cargo_layout.rs', 'tools/paddock-dev/src/check/c120.rs']);
const SCOPE = /^(?:crates|tools|apps|scripts)\//;
const VENDORED = /^apps\/[^/]+\/vendor\//;
const WAIVER = /conformance:\s*layout-guess\s*[—–-]+\s*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*layout-guess/;

const START = '(?:current_exe\\s*\\(|\\bOUT_DIR\\b)';
/** A `deps` path segment: in the middle (`/deps/`) or at the end of a path (`/deps"`, `/deps` at the end of a line). */
const DEPS = '[/\\\\]deps(?:[/\\\\]|["\'`)\\r\\n]|$)';
/** The token (up to the last white space or quote) before `index` on its line: a URL or a `node_modules/` path is not a layout. */
const TOKEN_TAIL = /[^\s"'`]*$/;
const NOT_A_LAYOUT = /https?:\/\/|node_modules[/\\]/;

export const RUST_PATTERNS = [
  { src: `${START}[^;]*?\\.parent\\s*\\([^;]*?\\.parent\\s*\\(`, what: 'reaches the profile folder by two or more `.parent()` hops from `current_exe()` or `OUT_DIR`' },
  { src: `${START}[^;]*?\\.ancestors\\s*\\(\\)[^;]*?\\.nth\\s*\\(\\s*(?:[2-9]|[1-9][0-9]+)\\s*\\)`, what: 'reaches the profile folder by `.ancestors().nth(n)` with n of 2 or more from `current_exe()` or `OUT_DIR`' },
  { src: `${START}[^;]*?(?:"\\.\\.[/\\\\]+\\.\\."|"\\.\\."[^;]*?"\\.\\.")`, what: 'reaches the profile folder by two or more `..` hops from `current_exe()` or `OUT_DIR`' },
  { src: DEPS + '|\\.(?:join|push)\\s*\\(\\s*"deps"\\s*\\)', what: 'names the `deps` folder of a build folder' },
];
export const SHELL_PATTERNS = [
  { src: '\\bOUT_DIR\\b[^\\n]*?(?:\\.\\.[/\\\\]+\\.\\.|dirname[^\\n]*dirname)|dirname[^\\n]*dirname[^\\n]*\\bOUT_DIR\\b', what: 'reaches the profile folder by two or more hops from `OUT_DIR`' },
  { src: DEPS, what: 'names the `deps` folder of a build folder' },
];
const lineAt = (text, index) => text.slice(0, index).split('\n').length;

/** `text` with the lines that start with `prefix` (after white space) emptied and a trailing comment (white space, then `prefix`) cut off; the line count stays. */
export function withoutCommentLines(text, prefix) {
  const trailing = new RegExp(`\\s${prefix === '#' ? '#' : '//'}[^\\n]*$`);
  return text
    .split('\n')
    .map((l) => (l.trim().startsWith(prefix) ? '' : l.replace(trailing, '')))
    .join('\n');
}

/** The layout guesses in one source: [{ line, what, waived: 'reason' | 'no-reason' | undefined }]. `shell` picks the pattern set. */
export function layoutGuesses(text, shell = false) {
  const code = withoutCommentLines(text, shell ? '#' : '//');
  const lines = text.split('\n');
  const hits = [];
  (shell ? SHELL_PATTERNS : RUST_PATTERNS).forEach(({ src, what }, order) => {
    for (const m of code.matchAll(new RegExp(src, 'g'))) hits.push({ index: m.index, order, what });
  });
  hits.sort((a, b) => a.index - b.index || a.order - b.order);
  const seen = new Set();
  const out = [];
  for (const { index, what } of hits) {
    const lineStart = code.lastIndexOf('\n', index - 1) + 1;
    if (NOT_A_LAYOUT.test(TOKEN_TAIL.exec(code.slice(lineStart, index))[0])) continue;
    const line = lineAt(code, index);
    if (seen.has(line)) continue;
    seen.add(line);
    const near = [lines[line - 1] ?? '', lines[line - 2] ?? ''].join('\n');
    out.push({ line, what, waived: WAIVER.test(near) ? 'reason' : BARE_WAIVER.test(near) ? 'no-reason' : undefined });
  }
  return out;
}

export default {
  id: 'C120',
  scope: 'both',
  mode: 'report-only',
  title: 'No guessing of the cargo build-folder layout',
  rule: 40,
  run(ctx) {
    const out = [];
    const fix = 'Use `paddock_env::cargo_layout` (`paddock_testkit::cargo_layout` in tests), which reads both cargo layouts and returns `None` for any other. A justified case carries `// conformance: layout-guess — <reason>`.';
    for (const f of ctx.files()) {
      const shell = f.endsWith('.sh');
      if ((!shell && !f.endsWith('.rs')) || !SCOPE.test(f) || VENDORED.test(f) || IMPLEMENTATION.has(f)) continue;
      const text = ctx.read(f);
      if (!text || !/current_exe|OUT_DIR|deps/.test(text)) continue;
      for (const g of layoutGuesses(text, shell)) {
        if (g.waived === 'reason') continue;
        if (g.waived === 'no-reason') out.push(warn('C120', 40, f, g.line, 'the layout-guess waiver gives no reason', 'Write `// conformance: layout-guess — <why this cannot be the other cargo layout>`.'));
        else out.push(warn('C120', 40, f, g.line, `${g.what}: right in one cargo layout, a wrong folder in the other`, fix));
      }
    }
    return out;
  },
};

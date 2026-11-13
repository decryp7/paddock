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
// followed; a block comment is read as code. A justified site carries `// conformance: layout-guess — <why>`
// (`# conformance: ...` in shell) on the line or the line above; the test code of a file, `// conformance: file-layout-guess — <why>` anywhere in it: it covers the lines inside `#[cfg(test)] mod` blocks, or every line of a file under `tests/`; production code in the same file is still reported.
// Test code is recognised by the plain `#[cfg(test)] mod name { ... }` only: `#[cfg(all(test, ...))]` and an out-of-line `mod tests;` (its own file, not under `tests/`) are not, so their sites stay reported (fail-safe).
// A waived site is not a finding: the host counts it as waived (`inlineWaiver`), so suppressions stay visible (ER-33). Rust host: tools/paddock-dev/src/check/c120.rs (same findings, line for line).
import { applyInlineWaiver, commentView } from '../util/inline-waiver.mjs';
import { warn } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

/** The files that read both layouts, and the check's own host port (its docs and tests name the patterns). */
export const IMPLEMENTATION = new Set(['crates/paddock-env/src/cargo_layout.rs', 'crates/paddock-testkit/src/cargo_layout.rs', 'tools/paddock-dev/src/check/c120.rs']);
const SCOPE = /^(?:crates|tools|apps|scripts)\//;
const VENDORED = /^apps\/[^/]+\/vendor\//;

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

/** The 1-based [first, last] line ranges of the `#[cfg(test)] mod name { ... }` blocks of a Rust source. */
export function testModuleLines(text) {
  const masked = maskRust(text);
  const ranges = [];
  for (const m of masked.matchAll(/#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*mod\s+\w+\s*\{/g)) {
    let depth = 0;
    let end = masked.length - 1;
    for (let i = m.index + m[0].length - 1; i < masked.length; i++) {
      if (masked[i] === '{') depth++;
      else if (masked[i] === '}' && --depth === 0) {
        end = i;
        break;
      }
    }
    ranges.push([lineAt(masked, m.index), lineAt(masked, end)]);
  }
  return ranges;
}

/** The layout guesses in one source: [{ line, what, waived: 'reason' | 'no-reason' | undefined, reason }]. `shell` picks the pattern set. */
export function layoutGuesses(text, shell = false, file = '') {
  const code = withoutCommentLines(text, shell ? '#' : '//');
  const view = commentView(text, shell ? 'hash' : 'rust');
  const hits = [];
  (shell ? SHELL_PATTERNS : RUST_PATTERNS).forEach(({ src, what }, order) => {
    for (const m of code.matchAll(new RegExp(src, 'g'))) hits.push({ index: m.index, order, what });
  });
  hits.sort((a, b) => a.index - b.index || a.order - b.order);
  const fileWaiver = view.file('file-layout-guess');
  const underTests = /(?:^|\/)tests\//.test(file);
  const testLines = fileWaiver.state === 'reason' && !shell && !underTests ? testModuleLines(text) : [];
  const fileWaives = (line) => underTests || testLines.some(([a, b]) => line >= a && line <= b);
  const seen = new Set();
  const out = [];
  for (const { index, what } of hits) {
    const lineStart = code.lastIndexOf('\n', index - 1) + 1;
    if (NOT_A_LAYOUT.test(TOKEN_TAIL.exec(code.slice(lineStart, index))[0])) continue;
    const line = lineAt(code, index);
    if (seen.has(line)) continue;
    seen.add(line);
    const site = view.waiver(line, 'layout-guess');
    out.push({ line, what, waived: site.state !== 'reason' && fileWaiver.state === 'reason' && fileWaives(line) ? fileWaiver : site });
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
      for (const g of layoutGuesses(text, shell, f)) {
        const site = warn('C120', 40, f, g.line, `${g.what}: right in one cargo layout, a wrong folder in the other`, fix);
        out.push(applyInlineWaiver(g.waived, site, () => warn('C120', 40, f, g.line, 'the layout-guess waiver gives no reason', 'Write `// conformance: layout-guess — <why this cannot be the other cargo layout>`.')));
      }
    }
    return out;
  },
};

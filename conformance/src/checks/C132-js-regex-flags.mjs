// C132: a regular expression in a JavaScript check that has a Rust twin carries the `u` flag (ER-21 class "the two hosts of a check
// diverge", found by the shared parity corpus, conformance/test/parity-corpus). Without `u` JavaScript reads text as UTF-16 code units:
// an astral character or emoji counts as two characters in `.`, `[^x]` and `{0,5}`, and a case-insensitive match does not fold `ſ` or
// the Kelvin sign as the Rust `regex` crate (Unicode on by default) does. The twin then finds what the original does not, or the other
// way round. With `u` both read code points.
// Read: the checks `conformance/src/checks/C<n>-*.mjs` whose twin `tools/paddock-dev/src/check/c<n>.rs` exists. Flagged: a regular
// expression literal without `u`, and `new RegExp(pattern)` or `new RegExp(pattern, flags)` with literal flags without `u`. A site that
// is fine (a pattern of ASCII only, which both hosts read alike) carries `// conformance: regex-u — <why>` on its line or the line above.
// Known gaps: a `RegExp` whose flags are not a literal, a regular expression built in another file, and division told from a
// regular expression only by the previous character (a heuristic: a miss is not reported, never a false site in a string or comment).
// ER-41: report-only (warnings) until its first green run on the real tree, recorded in docs/gates.toml.
import { warn } from '../finding.mjs';

const CHECK_FILE = /^conformance\/src\/checks\/C(\d+)-[^/]+\.mjs$/;
const WAIVER = /conformance:\s*regex-u\s*[—–-]+\s*\S.{2,}/;
const AFTER_KEYWORD = /(?:^|[^A-Za-z0-9_$.])(?:return|typeof|case|in|of|delete|void|throw|else|do|yield|await)$/;
const FIX = 'Add the `u` flag (and write the pattern for code points), then run the shared parity corpus (`tools/paddock-dev/tests/it/parity_corpus.rs`) over the check. A pattern of ASCII only carries `// conformance: regex-u — <why>`.';

/** [{ line, text }] of every regular expression without the `u` flag in JavaScript source `text`. */
export function regexWithoutU(text) {
  const out = [];
  const n = text.length;
  let prev = ''; // the last significant character
  let word = ''; // the identifier before it, for keywords
  const add = (at, what) => out.push({ line: lineOf(at), what });
  const lineOf = (at) => text.slice(0, at).split('\n').length;
  let i = 0;
  while (i < n) {
    const c = text[i];
    if (/\s/.test(c)) { i++; continue; }
    if (c === '/' && text[i + 1] === '/') { while (i < n && text[i] !== '\n') i++; continue; }
    if (c === '/' && text[i + 1] === '*') { const e = text.indexOf('*/', i + 2); i = e < 0 ? n : e + 2; continue; }
    if (c === '"' || c === "'" || c === '`') {
      i++;
      while (i < n && text[i] !== c) { if (text[i] === '\\') i++; i++; }
      i++; prev = c; word = ''; continue;
    }
    if (c === '/') {
      const regexAllowed = prev === '' || '(,=:[!&|?{};+-*%<>~^'.includes(prev) || AFTER_KEYWORD.test(word);
      if (regexAllowed) {
        const start = i;
        i++;
        let inClass = false;
        while (i < n && text[i] !== '\n') {
          if (text[i] === '\\') i += 2;
          else if (text[i] === '[') { inClass = true; i++; }
          else if (text[i] === ']') { inClass = false; i++; }
          else if (text[i] === '/' && !inClass) break;
          else i++;
        }
        i++;
        const f = /^[a-z]*/.exec(text.slice(i))[0];
        i += f.length;
        if (!f.includes('u')) add(start, 'a regular expression literal');
        prev = ')'; word = ''; continue;
      }
    }
    if (/[A-Za-z_$]/.test(c)) {
      const m = /^[A-Za-z0-9_$]+/.exec(text.slice(i))[0];
      if (m === 'RegExp' && /new\s+$/.test(text.slice(Math.max(0, i - 8), i))) {
        // new RegExp(pattern[, flags])
        let j = text.indexOf('(', i + 6);
        if (j >= 0) {
          let depth = 0, k = j, commas = [];
          for (; k < n; k++) {
            const ch = text[k];
            if (ch === '"' || ch === "'" || ch === '`') { const q = ch; k++; while (k < n && text[k] !== q) { if (text[k] === '\\') k++; k++; } continue; }
            if (ch === '(' || ch === '[' || ch === '{') depth++;
            else if (ch === ')' || ch === ']' || ch === '}') { depth--; if (depth === 0) break; }
            else if (ch === ',' && depth === 1) commas.push(k);
          }
          const flags = commas.length === 0 ? '' : /^\s*(["'`])([a-z]*)\1\s*,?\s*$/.exec(text.slice(commas[0] + 1, commas[1] ?? k));
          if (flags === '' || (flags && !flags[2].includes('u'))) add(i, 'new RegExp');
        }
      }
      i += m.length; prev = m[m.length - 1]; word = m; continue;
    }
    prev = c; word = ''; i++;
  }
  return out;
}

export default {
  id: 'C132',
  scope: 'root',
  mode: 'report-only',
  title: 'A regular expression of a check with a Rust twin has the u flag',
  rule: 21,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      const m = CHECK_FILE.exec(f);
      if (!m || !ctx.exists(`tools/paddock-dev/src/check/c${m[1]}.rs`)) continue;
      const text = ctx.read(f);
      if (text === null || text === undefined) continue;
      const lines = text.split('\n');
      for (const { line, what } of regexWithoutU(text)) {
        if (WAIVER.test(lines[line - 1] ?? '') || WAIVER.test(lines[line - 2] ?? '')) continue;
        out.push(warn('C132', 21, f, line, `${what} without the \`u\` flag in a check that has a Rust twin: an astral character or a folded letter is read differently by the two hosts`, FIX));
      }
    }
    return out;
  },
};

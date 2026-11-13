// Inline waivers, one mechanism for every check (ER-33): a comment `// conformance: <tag> — <reason>` on a site's line or the line
// above waives that site. The runner lists every waived site in the report's `waived` with the reason, the file and the line
// (`expires` is 'inline': there is no date), so suppressions stay visible. A check no longer carries its own waiver regexes: it
// passes the lines it considers "near" the site and its tag, and builds its finding through `applyInlineWaiver`.
// Rust host: tools/paddock-dev/src/check/inline_waiver.rs (the parity test compares the two).
//
// The marker, the tag, the dashes and the reason are on one line (spaces and tabs only between them): a dash at the end of a line
// does not take the next line as its reason, and a list dash on the next line is no dash. A marker without a reason is not a waiver: the site stays a finding, with the check's own text.

import { rustComments } from './mask-rust.mjs';
import { shellCommentRanges } from './shell-lexer.mjs';
import { yamlCommentRanges } from './yaml-lexer.mjs';

const blank = (s) => s.replace(/[^\n]/g, ' ');

/**
 * `text` with everything but its comments blanked (newlines kept, so lines and columns stay). `kind` is 'rust' (`//` and nested block
 * comments; not the inside of any literal), 'shell' (a `#` that starts a word; not in a quote, `$'..'`, `${..}` or a here-document) or 'yaml'
 * (a `#` after white space outside a scalar; not in a block scalar). A waiver marker anywhere else is data, not a waiver (ER-33): the readers
 * below look only at this text. The Rust host reads the same files with rustc's lexer and tree-sitter (tools/paddock-dev/src/check/lex).
 */
export function commentsOf(text, kind = 'rust') {
  if (kind === 'rust') return rustComments(text);
  const ranges = kind === 'shell' ? shellCommentRanges(text) : yamlCommentRanges(text);
  let out = '';
  let at = 0;
  for (const [a, b] of ranges) {
    out += blank(text.slice(at, a)) + text.slice(a, b);
    at = b;
  }
  return out + blank(text.slice(at));
}

/** The comments of one file, read by line: what a check asks to find out whether a site is waived. */
export function commentView(text, kind = 'rust') {
  const comments = commentsOf(text, kind);
  const lines = comments.split('\n');
  const near = (line) => [lines[line - 1] ?? '', lines[line - 2] ?? ''].join('\n');
  return {
    /** The waiver comment for `tag` on the 1-based `line` or the line above. */
    waiver: (line, tag) => inlineWaiver(near(line), tag),
    /** The waiver comment for `tag` anywhere in the file (a file-level marker). */
    file: (tag) => inlineWaiver(comments, tag),
  };
}

/** `{ state: 'reason', reason }`, `{ state: 'no-reason' }` or `{ state: 'none' }` for the waiver comment of `tag` in `near`. */
export function inlineWaiver(near, tag) {
  const reason = new RegExp(`conformance:[ \\t]*${tag}[ \\t]*[—–-]+[ \\t]*(\\S.{2,})`).exec(near);
  if (reason) return { state: 'reason', reason: reason[1] };
  return { state: new RegExp(`conformance:[ \\t]*${tag}`).test(near) ? 'no-reason' : 'none' };
}

/** `site` marked `inlineWaiver: <reason>` (the runner lists it as waived), `bare()` for a comment without a reason, else `site`. */
export function applyInlineWaiver(waiver, site, bare) {
  if (waiver.state === 'reason') return { ...site, inlineWaiver: waiver.reason };
  return waiver.state === 'no-reason' ? bare() : site;
}

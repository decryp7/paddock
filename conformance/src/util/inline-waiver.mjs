// Inline waivers, one mechanism for every check (ER-33): a comment `// conformance: <tag> — <reason>` on a site's line or the line
// above waives that site. The runner lists every waived site in the report's `waived` with the reason, the file and the line
// (`expires` is 'inline': there is no date), so suppressions stay visible. A check no longer carries its own waiver regexes: it
// passes the lines it considers "near" the site and its tag, and builds its finding through `applyInlineWaiver`.
// Rust host: tools/paddock-dev/src/check/inline_waiver.rs (the parity test compares the two).
//
// The marker, the tag, the dashes and the reason are on one line (spaces and tabs only between them): a dash at the end of a line
// does not take the next line as its reason, and a list dash on the next line is no dash. A marker without a reason is not a waiver: the site stays a finding, with the check's own text.

import { rustComments } from './mask-rust.mjs';

const blank = (s) => s.replace(/[^\n]/g, ' ');

/**
 * `text` with everything but its comments blanked (newlines kept, so lines and columns stay). `kind` is 'rust' (`//` and block
 * comments, not the inside of a string, raw string or char literal) or 'hash' (shell, YAML, TOML: a `#` at the start of a line or after
 * white space, outside a quoted string of the same line). A waiver marker in a string literal is data, not a waiver (ER-33): the
 * readers below look only at this text. Rust host: tools/paddock-dev/src/check/inline_waiver.rs.
 */
export function commentsOf(text, kind = 'rust') {
  if (kind === 'hash') return text.split('\n').map(hashComment).join('\n');
  return rustComments(text);
}

function hashComment(line) {
  let quote = '';
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote) {
      if (c === '\\' && quote === '"') i++;
      else if (c === quote) quote = '';
    } else if (c === '"' || c === "'") quote = c;
    else if (c === '#' && (i === 0 || /\s/.test(line[i - 1]))) return blank(line.slice(0, i)) + line.slice(i);
  }
  return blank(line);
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

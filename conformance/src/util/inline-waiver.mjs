// Inline waivers, one mechanism for every check (ER-33): a comment `// conformance: <tag> — <reason>` on a site's line or the line
// above waives that site. The runner lists every waived site in the report's `waived` with the reason, the file and the line
// (`expires` is 'inline': there is no date), so suppressions stay visible. A check no longer carries its own waiver regexes: it
// passes the lines it considers "near" the site and its tag, and builds its finding through `applyInlineWaiver`.
// Rust host: tools/paddock-dev/src/check/inline_waiver.rs (the parity test compares the two).
//
// The marker, the tag, the dashes and the reason are on one line (spaces and tabs only between them): a dash at the end of a line
// does not take the next line as its reason, and a list dash on the next line is no dash. A marker without a reason is not a waiver: the site stays a finding, with the check's own text.

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

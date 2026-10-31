// C53: nothing is ended by a bare pid (ADR 0033, rule 53). The OS hands a freed pid on (Windows at once), so a pid kept from a
// snapshot, a file or a parent link can name another program by the time it is signalled. Every kill goes through
// `paddock_proc` with a `ProcessId` (pid plus start time, checked before the kill) or through a handle or job object this
// process owns. The main check is Clippy's `disallowed-methods` (lint/clippy.toml and every workspace's clippy.toml:
// `libc::kill`, `libc::killpg`, nix's `kill`/`killpg`, sysinfo's `Process::kill`/`kill_with`, resolved by path, so a renamed
// import cannot hide one). This check covers what Clippy cannot see, in any Rust file outside the one implementation
// (`crates/paddock-proc/src/sys.rs`):
// - programs that kill by pid, named in string literals: `taskkill` (also `taskkill.exe`; `/T` follows parent pids that may
//   have been handed on), `Stop-Process`, `/bin/kill`, and a `kill`/`pkill` command;
// - Win32 calls: `TerminateProcess(`, `TerminateJobObject(` (fine on a handle the code opened and checked; say so in a
//   waiver);
// - a blanket `#![allow(clippy::disallowed_methods)]` (also inside `#![cfg_attr(.., allow(..))]`): an inner attribute
//   turns the lint off for a whole crate or module, so Clippy cannot see a bare-pid kill in it. Allow or expect the lint on
//   the item that needs it instead;
// - open follow-ups: a comment or `#[expect]` reason naming `ADR 0033 WP-<n>` marks a known site (a bare-pid kill waived
//   for Clippy, or a walk that follows parent pids without start times). Each needs a dated waiver in
//   paddock.waivers.toml, so the follow-up cannot be forgotten.
// Limits: a pid passed to another program by other means (a script file, a shell string built at run time) is not seen.
// A justified site carries `// conformance: pid-kill — <reason>` on the line or the line above.
import { error } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

const WAIVER = /conformance:\s*pid-kill\s*[—–-]+\s*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*pid-kill/;
/** The one place that may signal a pid: it does so right after the identity check. */
const IMPLEMENTATION = new Set(['crates/paddock-proc/src/sys.rs']);

const PATTERNS = [
  { re: /\bTerminateProcess\s*\(/g, what: 'ends a process with `TerminateProcess`' },
  { re: /\bTerminateJobObject\s*\(/g, what: 'ends a job with `TerminateJobObject`' },
];
/** Programs that kill by pid; matched on the unmasked text, since their names are string literals. */
const PROGRAMS = [
  { re: /"(taskkill)(?:\.exe)?"/g, quote: 0 },
  { re: /"[^"\n]*\b(Stop-Process)\b[^"\n]*"/g, quote: 0 },
  { re: /"(\/(?:usr\/)?bin\/kill)"/g, quote: 0 },
  { re: /\b(?:command|Command::new)\s*\(\s*"(kill|pkill)"/g, quote: -1 },
];
/** A crate- or module-wide allow of the lint that bans bare-pid kills. */
const BLANKET = /#!\[(?:cfg_attr\([^\]]*?)?allow\([^\]]*\bclippy::disallowed_methods\b/g;
/** An open ADR 0033 follow-up, in a comment or an `#[expect]` reason. */
const FOLLOW_UP = /ADR 0033 (WP-\d+)/g;

const lineAt = (text, index) => text.slice(0, index).split('\n').length;

/** The pid kills in one Rust source: [{ line, what, waived: 'reason' | 'no-reason' | undefined }]. */
export function pidKills(text) {
  const masked = maskRust(text);
  const lines = text.split('\n');
  const hits = [];
  for (const { re, what } of PATTERNS) for (const m of masked.matchAll(re)) hits.push({ index: m.index, what });
  for (const { re, quote } of PROGRAMS) {
    for (const m of text.matchAll(re)) {
      // Only a literal in code: in a comment or inside another string the mask has blanked the quote.
      const q = quote === 0 ? m.index : m.index + m[0].lastIndexOf('"');
      if (masked[q] === '"') hits.push({ index: m.index, what: `runs \`${m[1]}\`, which ends processes by pid` });
    }
  }
  for (const m of masked.matchAll(BLANKET)) hits.push({ index: m.index, what: 'allows `clippy::disallowed_methods` for a whole crate or module, which hides bare-pid kills from Clippy' });
  for (const m of text.matchAll(FOLLOW_UP)) hits.push({ index: m.index, what: `an open ADR 0033 follow-up (${m[1]}): a known bare-pid kill or parent-pid walk`, followUp: true });
  return hits
    .sort((a, b) => a.index - b.index)
    .map(({ index, what, followUp }) => {
      const line = lineAt(masked, index);
      const near = [lines[line - 1] ?? '', lines[line - 2] ?? ''].join('\n');
      // A follow-up marker is waived only in paddock.waivers.toml (dated), never inline.
      const waived = followUp ? undefined : WAIVER.test(near) ? 'reason' : BARE_WAIVER.test(near) ? 'no-reason' : undefined;
      return { line, what, waived };
    });
}

export default {
  id: 'C53',
  title: 'Nothing is ended by a bare pid',
  rule: 53,
  run(ctx) {
    const out = [];
    const fix =
      'End it through `paddock_proc` with a `ProcessId` (`kill_tree`, `end`, `terminate`; `Supervised` for a child), or through a handle or job object this process owns (ADR 0033). A justified case carries `// conformance: pid-kill — <reason>`.';
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs') || IMPLEMENTATION.has(f)) continue;
      const text = ctx.read(f);
      if (!text || !/kill|Stop-Process|Terminate(?:Process|JobObject)|ADR 0033 WP-|disallowed_methods/.test(text)) continue;
      for (const k of pidKills(text)) {
        if (k.waived === 'reason') continue;
        if (k.waived === 'no-reason') out.push(error('C53', 53, f, k.line, 'the pid-kill waiver gives no reason', 'Write `// conformance: pid-kill — <why this pid cannot have been handed on>`.'));
        else if (k.what.startsWith('an open') || k.what.startsWith('allows')) out.push(error('C53', 53, f, k.line, k.what, 'Fix the site through paddock_proc (ADR 0033), or give it a dated waiver in paddock.waivers.toml.'));
        else out.push(error('C53', 53, f, k.line, `${k.what}: the pid may belong to another process by now`, fix));
      }
    }
    return out;
  },
};

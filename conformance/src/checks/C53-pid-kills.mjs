// C53: nothing is ended by a bare pid (ADR 0033). The OS hands a freed pid on (Windows at once), so a pid kept from a
// snapshot, a file or a parent link can name another program by the time it is signalled: Paddock's Windows CI killed one
// test's browser network service while ending another test's browser. Every kill goes through `paddock_proc` with a
// `ProcessId` (pid plus start time, checked before the kill) or through a handle or job object this process owns. Flagged,
// in any Rust file outside the one implementation (`crates/paddock-proc/src/sys.rs`):
// - `libc::kill(`, nix's `signal::kill(`, `killpg(` (a signal to a pid or a process group);
// - `TerminateProcess(`, `TerminateJobObject(` (fine on a handle the code opened and checked; say so in a waiver);
// - `taskkill` (any `"taskkill"` or `"taskkill.exe"` in code: `/T` follows parent pids that may have been handed on) and a
//   `kill` or `pkill` command;
// - sysinfo's `.kill()` / `.kill_with(..)` on a process looked up by pid (`.process(Pid::from_u32(..))`, `Pid::from(..)`).
// A justified site carries `// conformance: pid-kill — <reason>` on the line or the line above.
import { error } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

const WAIVER = /conformance:\s*pid-kill\s*[—–-]+\s*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*pid-kill/;
/** The one place that may signal a pid: it does so right after the identity check. */
const IMPLEMENTATION = new Set(['crates/paddock-proc/src/sys.rs']);

const PATTERNS = [
  { re: /\blibc::kill\s*\(/g, what: 'signals a bare pid (`libc::kill`)' },
  { re: /\bsignal::kill\s*\(/g, what: 'signals a bare pid (`signal::kill`)' },
  { re: /\bkillpg\s*\(/g, what: 'signals a process group by a bare pid (`killpg`)' },
  { re: /\bTerminateProcess\s*\(/g, what: 'ends a process with `TerminateProcess`' },
  { re: /\bTerminateJobObject\s*\(/g, what: 'ends a job with `TerminateJobObject`' },
  { re: /\.kill_with\s*\(/g, what: "ends a process found by pid (sysinfo's `kill_with`)" },
];
/** Programs that kill by pid; matched on the unmasked text, since their names are string literals. */
const PROGRAMS = [
  { re: /"(taskkill)(?:\.exe)?"/g, quote: 0 },
  { re: /\b(?:command|Command::new)\s*\(\s*"(kill|pkill)"/g, quote: -1 },
];
/** sysinfo: a process looked up by a raw pid and killed in the same statement or the `if let` that follows. */
const SYSINFO_LOOKUP = /\.process\s*\(\s*(?:sysinfo::)?Pid::from(?:_u32)?\s*\([^)]*\)\s*\)/g;

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
  for (const m of masked.matchAll(SYSINFO_LOOKUP)) {
    // The lookup, then `.kill()` on it within the next few lines (`if let Some(p) = sys.process(..) { p.kill(); }`).
    const after = masked.slice(m.index, m.index + 240);
    if (/\.kill\s*\(\s*\)/.test(after)) hits.push({ index: m.index, what: "ends a process looked up by a bare pid (sysinfo's `kill`)" });
  }
  return hits
    .sort((a, b) => a.index - b.index)
    .map(({ index, what }) => {
      const line = lineAt(masked, index);
      const near = [lines[line - 1] ?? '', lines[line - 2] ?? ''].join('\n');
      const waived = WAIVER.test(near) ? 'reason' : BARE_WAIVER.test(near) ? 'no-reason' : undefined;
      return { line, what, waived };
    });
}

export default {
  id: 'C53',
  title: 'Nothing is ended by a bare pid',
  rule: 40,
  run(ctx) {
    const out = [];
    const fix =
      'End it through `paddock_proc` with a `ProcessId` (`kill_tree`, `end`, `terminate`; `Supervised` for a child), or through a handle or job object this process owns (ADR 0033). A justified case carries `// conformance: pid-kill — <reason>`.';
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs') || IMPLEMENTATION.has(f)) continue;
      const text = ctx.read(f);
      if (!text || !/kill|Terminate(?:Process|JobObject)/.test(text)) continue;
      for (const k of pidKills(text)) {
        if (k.waived === 'reason') continue;
        if (k.waived === 'no-reason') out.push(error('C53', 40, f, k.line, 'the pid-kill waiver gives no reason', 'Write `// conformance: pid-kill — <why this pid cannot have been handed on>`.'));
        else out.push(error('C53', 40, f, k.line, `${k.what}: the pid may belong to another process by now`, fix));
      }
    }
    return out;
  },
};

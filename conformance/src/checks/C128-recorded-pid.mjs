// C128: a test turns a pid it read from a file or a child's output into an identity with `ProcessId::of` (rule 53, ADR 0033; the ER-21
// class "a test reads a pid from a file or stdout, then observes or acts on it after the code under test may have ended it": Spotter's
// helper test in CI run 8118, f224dd0b4, and its review). By the time the test looks, the writer may have ended: `ProcessId::of` then
// gives `None`, which a poll reads as "not yet" (a false timeout, Spotter's 10 s) or an `unwrap` turns into a panic; and when the pid
// was handed on it gives the identity of whoever holds it now, which a failure path then ends (ER-14). Use
// `paddock_testkit_start::recorded`: `read_pid_file` / `look_pid_file` (the file's modification time bounds the holder's start) and
// `printed` (a stamp taken before the spawn), which answer `Recorded::Ended`, never "not yet" and never the next holder.
//
// Flagged, in Rust test code only (test files, and a file's `#[cfg(test)]` module onwards), with comments and string insides masked:
// `ProcessId::of(` (or `ProcessId::of` passed as a function) whose statement parses text (`.parse`), or whose argument is a name bound
// earlier in the same function by a `let` whose statement parses text. A pid from a `Child` the test holds (`child.id()`) is not
// flagged: it cannot be handed on before the test reaps it.
// Known gaps (not found): a pid parsed in another function and passed in, and a pid parsed with something other than `.parse`.
// A site that is fine carries `// conformance: recorded-pid — <why>` on its line or the line above.
// ER-41: report-only (warnings) until its first green run on the real tree, recorded in docs/gates.toml.
import { warn } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

const WAIVER = /conformance:\s*recorded-pid\s*[—–-]+\s*\S.{2,}/;
const SKIP_PATH = /(?:^|\/)(?:target|vendor)\/|(?:^|\/)fixtures?\/|^crates\/paddock-testkit-start\/src\/recorded\.rs$|^crates\/paddock-proc\/src\/identity\.rs$/;
const RUST_TEST_FILE = /(?:^|\/)(?:tests|benches)\/.+\.rs$|(?:^|\/)tests\.rs$|_tests?\.rs$/;
const CFG_TEST_MOD = /#\[cfg\((?:all\()?test[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{/;
const SITE = /\bProcessId::of\b(?!_)/g;
const PARSES = /\.parse\b/;
const FIX = 'Read it with `paddock_testkit_start::recorded::read_pid_file` / `look_pid_file` (a pid file) or `printed` (a pid a child printed, with a `Stamp` taken before the spawn): they answer `Recorded::Ended` when the writer is gone, never "not yet" and never the process that got the pid next. A site that is fine carries `// conformance: recorded-pid — <why>`.';

const escape = (t) => t.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** The statement around `at`: from just past the last `;`, `{` or `}` before it to the next `;` (or the end). */
function statement(masked, at) {
  const start = Math.max(masked.lastIndexOf(';', at - 1), masked.lastIndexOf('{', at - 1), masked.lastIndexOf('}', at - 1)) + 1;
  const end = masked.indexOf(';', at);
  return masked.slice(start, end < 0 ? masked.length : end);
}

/** The single name an `of(` call is given (`pid`, `*pid`, `&pid`), or null. */
function argName(masked, at) {
  const m = /^ProcessId::of\s*\(\s*[*&]?\s*([A-Za-z_]\w*)\s*\)/.exec(masked.slice(at));
  return m ? m[1] : null;
}

/** The sites of one file: [{ line }]. Empty for a file that is not Rust test code. */
export function recordedPidSites(text, file) {
  if (!file.endsWith('.rs') || SKIP_PATH.test(file)) return [];
  const masked = maskRust(text);
  let from = 0;
  if (!RUST_TEST_FILE.test(file)) {
    const m = CFG_TEST_MOD.exec(masked);
    if (!m) return [];
    from = m.index;
  }
  const lines = text.split('\n');
  const lineAt = (i) => text.slice(0, i).split('\n').length;
  const out = [];
  for (const m of masked.matchAll(SITE)) {
    if (m.index < from) continue;
    let parsed = PARSES.test(statement(masked, m.index));
    const name = parsed ? null : argName(masked, m.index);
    if (name) {
      const fnStart = Math.max(from, masked.lastIndexOf('fn ', m.index));
      const scope = masked.slice(fnStart, m.index);
      const lets = [...scope.matchAll(new RegExp(`\\blet\\s+(?:mut\\s+)?(?:[^=;]*[(,\\s])?${escape(name)}\\b[^=;]*=`, 'g'))];
      const last = lets.at(-1);
      if (last) parsed = PARSES.test(statement(masked, fnStart + last.index + last[0].length));
    }
    if (!parsed) continue;
    const line = lineAt(m.index);
    if (WAIVER.test(lines[line - 1] ?? '') || WAIVER.test(lines[line - 2] ?? '')) continue;
    if (!out.some((o) => o.line === line)) out.push({ line });
  }
  return out;
}

export default {
  id: 'C128',
  scope: 'root',
  mode: 'report-only',
  title: 'Tests take a recorded pid\'s identity through the kit, never ProcessId::of',
  rule: 53,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs')) continue;
      const text = ctx.read(f);
      if (!text || !text.includes('ProcessId::of')) continue;
      for (const s of recordedPidSites(text, f)) {
        out.push(warn('C128', 53, f, s.line, 'takes the identity of a pid read from a file or a child\'s output with `ProcessId::of`: a writer the code under test already ended reads as "not yet", and a handed-on pid as its new holder (rule 53, ER-14)', FIX));
      }
    }
    return out;
  },
};

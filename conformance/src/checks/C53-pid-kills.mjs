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
// Liveness (user 2026-11-12, same rule, a baseline instead of an error for the sites that exist): asking whether a RAW pid
// is alive has the same reuse problem as ending it (the answer can be about another program), in tests as much as in product
// code: `presence(pid)` (the bare-bool `is_alive(pid)` / `may_be_alive(pid)` no longer exist in paddock-proc; the names still count for copies; also bare, in a file that imports them, and
// `crate::` inside paddock-proc), `kill(pid, 0)`, `OpenProcess(` and sysinfo's `Pid::from_u32(` lookups. Ask a
// `paddock_proc::ProcessId` (pid plus start time) instead: `id.presence()`, `id.has_ended()`, `id.may_be_alive()`. Existing sites sit in
// `conformance/baselines/C53.json` (file + hash of the normalised line; it may only shrink, the same helpers as C71's baseline);
// a new site needs a `// conformance: pid-liveness — <reason>` (for example: the pid came from a handle this process owns).
// Limits: a pid passed to another program by other means (a script file, a shell string built at run time) is not seen.
// A justified site carries `// conformance: pid-kill — <reason>` on the line or the line above.
import { error, warn } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';
import { againstBaseline, baselineGrowth, growthMovedWarning, lineHash, movedWarning, sourceGone } from './C71-test-timing.mjs';

export const LIVENESS_BASELINE = 'conformance/baselines/C53.json';
const LIVENESS_WAIVER = /conformance:\s*pid-liveness\s*[—–-]+\s*(\S.{2,})/;
const BARE_LIVENESS_WAIVER = /conformance:\s*pid-liveness/;
const LIVENESS_NAMES = 'is_alive|may_be_alive|presence';

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

/**
 * The raw-pid liveness checks in one Rust source: [{ line, what, waived }]. `file` decides what counts: the one implementation is
 * skipped, and a bare `is_alive(pid)` is the pid function only inside paddock-proc or where the file imports it from there.
 */
export function pidLiveness(text, file = '') {
  if (IMPLEMENTATION.has(file)) return [];
  const masked = maskRust(text);
  const lines = text.split('\n');
  const hits = [];
  const add = (re, what, ok = () => true) => {
    for (const m of masked.matchAll(re)) if (ok(m)) hits.push({ index: m.index, what: what(m) });
  };
  add(new RegExp(`\\bpaddock_proc::(${LIVENESS_NAMES})\\s*\\(`, 'g'), (m) => `asks \`paddock_proc::${m[1]}\` about a raw pid`);
  const inProc = file.startsWith('crates/paddock-proc/');
  add(new RegExp(`\\bcrate::(${LIVENESS_NAMES})\\s*\\(`, 'g'), (m) => `asks \`${m[1]}\` about a raw pid`, () => inProc);
  // Names this file imports from paddock_proc: `use paddock_proc::{a, b as c}`, `use paddock_proc::a as c`, `use paddock_proc::*`.
  const imported = new Set();
  for (const m of masked.matchAll(/\buse\s+paddock_proc::(?:\{([^}]*)\}|(\*|\w+(?:\s+as\s+\w+)?))/g)) {
    for (const item of (m[1] ?? m[2]).split(',').map((n) => n.trim())) {
      if (item === '*') for (const n of LIVENESS_NAMES.split('|')) imported.add(n);
      else {
        const [name, alias] = item.split(/\s+as\s+/);
        if (LIVENESS_NAMES.split('|').includes(name)) imported.add(alias ?? name);
      }
    }
  }
  const names = [...new Set([...LIVENESS_NAMES.split('|'), ...imported])].join('|');
  add(
    new RegExp(`(?<![\\w.:])(${names})\\s*\\(`, 'g'),
    (m) => `asks \`${m[1]}\` about a raw pid`,
    (m) => (inProc || imported.has(m[1])) && !/\bfn\s+$/.test(masked.slice(Math.max(0, m.index - 12), m.index)),
  );
  // Only `kill(pid, 0)` and nix's `kill(pid, None)` are liveness; a real signal is the kill check's.
  add(/\bkill\s*\([^;\n]*?,\s*(?:0|None)\s*\)/g, () => 'checks a raw pid with `kill(pid, 0)`', (m) => !/\.\s*$/.test(masked.slice(Math.max(0, m.index - 2), m.index)));
  add(/\bOpenProcess\s*\(/g, () => 'opens a process by its raw pid with `OpenProcess`');
  add(/\bPid::from(?:_u32|_usize)?\s*\((?!\s*(?:std::)?process::id\(\))/g, () => 'looks a raw pid up in sysinfo');
  return hits
    .sort((a, b) => a.index - b.index)
    .map(({ index, what }) => {
      const line = lineAt(masked, index);
      const near = [lines[line - 1] ?? '', lines[line - 2] ?? ''].join('\n');
      const waived = LIVENESS_WAIVER.test(near) ? 'reason' : BARE_LIVENESS_WAIVER.test(near) ? 'no-reason' : undefined;
      return { line, what, waived, text: lines[line - 1] ?? '' };
    });
}

export default {
  id: 'C53',
  scope: 'both',
  mode: 'gate',
  title: 'Nothing is ended by a bare pid',
  rule: 53,
  run(ctx) {
    const out = [];
    const fix =
      'End it through `paddock_proc` with a `ProcessId` (`kill_tree`, `end`, `terminate`; `Supervised` for a child), or through a handle or job object this process owns (ADR 0033). A justified case carries `// conformance: pid-kill — <reason>`.';
    for (const f of ctx.files()) {
      // Vendored third-party code (`apps/<app>/vendor/`, e.g. Dyno's patched sysinfo) is its upstream's, not this repository's.
      if (!f.endsWith('.rs') || IMPLEMENTATION.has(f) || /^apps\/[^/]+\/vendor\//.test(f)) continue;
      const text = ctx.read(f);
      if (!text || !/kill|Stop-Process|Terminate(?:Process|JobObject)|ADR 0033 WP-|disallowed_methods/.test(text)) continue;
      for (const k of pidKills(text)) {
        if (k.waived === 'reason') continue;
        if (k.waived === 'no-reason') out.push(error('C53', 53, f, k.line, 'the pid-kill waiver gives no reason', 'Write `// conformance: pid-kill — <why this pid cannot have been handed on>`.'));
        else if (k.what.startsWith('an open') || k.what.startsWith('allows')) out.push(error('C53', 53, f, k.line, k.what, 'Fix the site through paddock_proc (ADR 0033), or give it a dated waiver in paddock.waivers.toml.'));
        else out.push(error('C53', 53, f, k.line, `${k.what}: the pid may belong to another process by now`, fix));
      }
    }
    out.push(...livenessFindings(ctx));
    return out;
  },
};

function livenessFindings(ctx) {
  const out = [];
  const fix = 'Ask a `paddock_proc::ProcessId` (pid plus start time): `id.presence()` (or `id.has_ended()` to wait for an end, `id.may_be_alive()` before deleting or taking over). A justified case carries `// conformance: pid-liveness — <reason>`.';
  const sites = [];
  for (const f of ctx.files()) {
    if (!f.endsWith('.rs') || /^apps\/[^/]+\/vendor\//.test(f)) continue;
    const text = ctx.read(f);
    if (!text || !/is_alive|may_be_alive|presence|kill|OpenProcess|Pid::from/.test(text)) continue;
    for (const k of pidLiveness(text, f)) {
      if (k.waived === 'reason') continue;
      if (k.waived === 'no-reason') out.push(error('C53', 53, f, k.line, 'the pid-liveness waiver gives no reason', 'Write `// conformance: pid-liveness — <why this pid cannot have been handed on>`.'));
      else sites.push({ file: f, line: k.line, what: k.what, hash: lineHash(k.text) });
    }
  }
  const raw = ctx.read(LIVENESS_BASELINE);
  let entries = [];
  if (raw) {
    try {
      const parsed = JSON.parse(raw);
      if (!Array.isArray(parsed?.sites)) throw new Error('no sites list');
      entries = parsed.sites;
    } catch {
      // Fail closed with one error: the same on the Rust host (tools/paddock-dev/src/check/c53_liveness.rs).
      out.push(error('C53', 53, LIVENESS_BASELINE, 1, `${LIVENESS_BASELINE} is not JSON with a \`sites\` list: the liveness baseline cannot be read`, 'Restore the file from git (`git checkout origin/main -- conformance/baselines/C53.json`); never rewrite it by hand to add entries.'));
      return out;
    }
  }
  const { fresh, stale, moved } = againstBaseline(sites, entries, sourceGone(ctx));
  for (const m of moved) out.push(movedWarning('C53', 53, LIVENESS_BASELINE, m));
  for (const s of fresh) out.push(error('C53', 53, s.file, s.line, `${s.what}: the pid may belong to another process by now`, fix));
  for (const e of stale) out.push(warn('C53', 53, e.file, e.line ?? 1, `${LIVENESS_BASELINE} lists a liveness site that is gone (hash ${e.hash}); the baseline may only shrink`, 'Remove the entry.'));
  if (entries.length) {
    const g = baselineGrowth(ctx, entries, LIVENESS_BASELINE);
    if (g.reason) out.push(warn('C53', 53, LIVENESS_BASELINE, 1, `the baseline's shrink-only rule is not enforced here: ${g.reason}`, 'Fetch the target branch (`git fetch origin main`, full history), or set PADDOCK_BASE_REF to the ref to compare with.'));
    else for (const m of g.moved) out.push(growthMovedWarning('C53', 53, LIVENESS_BASELINE, m));
    if (!g.reason && g.added.length) out.push(error('C53', 53, LIVENESS_BASELINE, 1, `the baseline grew: ${g.added.length} entr${g.added.length === 1 ? 'y is' : 'ies are'} not in the baseline at the merge base ${g.base.slice(0, 9)}; it may only shrink`, 'Remove the entries and fix the sites through a ProcessId. Growing the baseline needs the user\'s approval.'));
  }
  return out;
}

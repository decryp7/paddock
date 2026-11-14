// C126 (ADR 0103; the number assigned by the coordinator on 2026-11-15): tests that run steps one after the other
// under one time bound, poll at a fixed pace, or start a process nothing ends (ER-21, ER-12, ER-14; ADR 0103; the class:
// CI runs 8050, 8060, 8061, 8066, 8090 and 8099 timed out on the loaded Mac). In test code only, with comments and string insides
// masked:
// - rule A (serial steps): a loop (`for`, `while`, Rust `loop`) whose body runs a program to completion itself (Rust `.output()`,
//   `.status()`, `.wait_with_output()`, in a file that builds a `Command`; Node `spawnSync(`, `execFileSync(`, `execSync(`). Every turn adds a program's
//   start and run to the one bound nextest or node --test gives the whole test, so a loaded host passes the sum later than any
//   single step. Run the cases side by side (`paddock_testkit::run_cases` / `both`), one test per case, or wait on progress
//   (`paddock_testkit::wait_progressing`, the Node kit's `waitProgressing`).
// - rule B (fixed poll, Node): a `setTimeout(..., <number>)` inside a loop: a fixed pace answers late (0.2 s per look in bounded.sh
//   made each of six phases last at least 0.2 s) or looks too often. Use `waitProgressing` (scripts/test-kit.mjs), which backs off.
//   Rust's fixed sleeps are C71's.
// - rule C (unguarded spawn): Rust `spawn_detached(`, or `.spawn()` in a function that names none of `Reaper`, `Supervised`,
//   `wait_exit`, `wait_ready_or_exit`, `.kill(`, `kill_tree`, `.wait(`, `wait_with_output`, `end_holders` (in a file that builds a
//   `Command`); Node `detached: true` in a
//   file that names no `Reaper`, `reapUnder` or `endHolders`. A process a failing test does not end outlives it (a `slot hold`
//   leaked from vm-lock.test.mjs, run 8099). Start it through `paddock_testkit_start::Reaper` or the Node kit's `Reaper`.
// A site that is fine carries `// conformance: test-steps — <why>` on the line or the line above (for rule A also on the loop's line
// or the line above it).
// ER-41: report-only (warnings) until its first green run on the real tree, recorded in docs/gates.toml; then flip SEVERITY and the
// mode to gate. The kits themselves (crates/paddock-testkit*, scripts/test-kit.mjs) and the conformance fixtures are skipped.
import { warn } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';
import { maskTs } from './C71-test-timing.mjs';

export const SEVERITY = warn;
const WAIVER = /conformance:\s*test-steps\s*[—–-]+\s*\S.{2,}/;
const SKIP_PATH = /(?:^|\/)(?:node_modules|dist|target|vendor)\/|(?:^|\/)fixtures?\/|^conformance\/test\/fixtures\/|(?:^|\/)crates\/paddock-testkit(?:-start)?\/|^scripts\/test-kit(?:\.test)?\.mjs$/;
const RUST_TEST_FILE = /(?:^|\/)(?:tests|benches)\/.+\.rs$|(?:^|\/)tests\.rs$|_tests?\.rs$/;
const CFG_TEST_MOD = /#\[cfg\((?:all\()?test[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{/;
const NODE_TEST_FILE = /(?:\.test\.(?:mjs|js|cjs|ts|mts)|(?:^|\/)[\w-]*test-support\.mjs|^conformance\/test\/(?:helpers|lib\/[\w-]+)\.mjs)$/;
const RUST_RUN = /\.(?:output|status|wait_with_output)\s*\(\s*\)/;
/** A Rust file that builds commands: `.status()`, `.output()` and `.spawn()` mean a program only there (not an HTTP status, an actor). */
const RUST_COMMANDS = /\bCommand::new\b|\bcommand\s*\(|\bquiet_command\b|\bTestCommand\b|\bCommand\b/;
const NODE_RUN = /\b(?:spawnSync|execFileSync|execSync)\s*\(/;
/** What makes a `.status()` a program's: a Command chain in the same statement (an HTTP response's `.status()` is not one). */
const COMMAND_CHAIN = /\bCommand::new\b|\bcommand\s*\(|\bquiet_command\b|\bTestCommand\b|\.args?\s*\(|\.current_dir\s*\(|\.envs?\s*\(|\.std(?:in|out|err)\s*\(/;
const COMMAND_MADE = /\bCommand::new\b|\bcommand\s*\(|\bquiet_command\b|\bTestCommand\b/;
/** A Node call that guards the file's detached children (a word alone is not one). */
const NODE_GUARD = /\bnew\s+Reaper\s*\(|\breapUnder\s*\(|\bendHolders\s*\(|\bfileReaper\s*\(/;
const ASSERTION = /\b(?:assert\w*|panic|unreachable|todo|unimplemented)!\s*\(/;

const escape = (t) => t.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** The start of the statement that holds `at`: just past the last `;`, `{` or `}` before it. */
function statementStart(masked, at) {
  return Math.max(masked.lastIndexOf(';', at - 1), masked.lastIndexOf('{', at - 1), masked.lastIndexOf('}', at - 1)) + 1;
}

/** Whether the `.status()` at `at` belongs to a Command: its statement is a Command chain, or its receiver was made from one. */
function commandStatus(masked, at) {
  const stmt = masked.slice(statementStart(masked, at), at);
  if (COMMAND_CHAIN.test(stmt)) return true;
  const recv = /(\w+)\s*$/.exec(stmt)?.[1];
  if (!recv) return false;
  return new RegExp(`\\blet\\s+(?:mut\\s+)?${escape(recv)}\\b[^;]*?=[^;]*?(?:${COMMAND_MADE.source})`).test(masked);
}

/**
 * Rule C for one Rust function body (`inner`, starting at `base`): each `.spawn()` of a Command must be waited for, ended or handed on
 * by name (`name.wait(`, `.kill(`, `.try_wait(`, `.wait_with_output(`, or a call that takes it: `wait_exit(name`, `adopt(&name`, ...),
 * with no assertion between the spawn and that guard (a failing assertion would leave it running). Returns [index, what] pairs.
 */
function unguardedSpawns(inner, base) {
  const out = [];
  for (const m of inner.matchAll(/\.spawn\s*\(\s*\)/g)) {
    // A nested fn is judged on its own.
    if (/\bfn\s+\w+/.test(inner.slice(0, m.index))) break;
    const start = statementStart(inner, m.index);
    const end = inner.indexOf(';', m.index);
    const rest = inner.slice(m.index, end < 0 ? inner.length : end);
    if (/\.(?:wait|wait_with_output|kill|try_wait|is_err)\s*\(/.test(rest)) continue; // waited for in the same statement, or meant to fail
    const head = inner.slice(start, m.index);
    const name = /\blet\s+(?:mut\s+)?(\w+)/.exec(head)?.[1];
    // Handed straight to a guard: a wrapper (`Reaped(c.spawn()…)`) or a struct field (`child: c.spawn()…`) owns it.
    if (/\b[A-Z]\w*\s*\(\s*[^()]*$/.test(head) || (!name && /\b\w+\s*:\s*[^:]*$/.test(head))) continue;
    if (!name) {
      out.push([base + m.index, 'starts a process and neither waits for it, ends it nor hands it to a Reaper']);
      continue;
    }
    const after = inner.slice(end < 0 ? inner.length : end);
    const n = escape(name);
    // Waited for, ended, or moved on by name: passed to a call, matched, or bound again.
    const guard = new RegExp(`\\b${n}\\s*\\.\\s*(?:wait|wait_with_output|kill|try_wait)\\s*\\(|[(,]\\s*(?:&\\s*(?:mut\\s+)?)?${n}\\s*[,)]|\\b(?:match|=)\\s*(?:&\\s*(?:mut\\s+)?)?${n}\\b`).exec(after);
    if (!guard) out.push([base + m.index, `starts a process (\`${name}\`) and neither waits for it, ends it nor hands it to a Reaper`]);
    else if (ASSERTION.test(after.slice(0, guard.index))) out.push([base + m.index, `starts a process (\`${name}\`) and asserts before its wait: a failing assertion leaves it running`]);
  }
  return out;
}

/** The index just past the brace that closes the `{` at `open` in `masked`, or -1. */
function closeBrace(masked, open) {
  let depth = 0;
  for (let i = open; i < masked.length; i++) {
    if (masked[i] === '{') depth++;
    else if (masked[i] === '}' && --depth === 0) return i + 1;
  }
  return -1;
}

/**
 * The body of the loop or function whose keyword is at `at`, as [start, end), or null. Skips a header's parentheses. A Node loop
 * without a block (`for (...) await pause(50);`) has the statement up to its `;` as body when `statement` is set.
 */
function bodyAfter(masked, at, statement = false) {
  let depth = 0;
  let headerEnd = -1;
  for (let i = at; i < masked.length; i++) {
    const c = masked[i];
    if (c === '(' || c === '[') depth++;
    else if (c === ')' || c === ']') {
      if (--depth === 0 && headerEnd < 0) headerEnd = i + 1;
    } else if (c === ';' && depth === 0) return statement && headerEnd > 0 ? [headerEnd, i + 1] : null;
    else if (c === '{' && depth === 0) {
      if (statement && headerEnd > 0 && masked.slice(headerEnd, i).trim() !== '') continue;
      const end = closeBrace(masked, i);
      return end < 0 ? null : [i, end];
    }
  }
  return null;
}

/** The sites of one file: [{ index, line, rule: 'A' | 'B' | 'C', what }]. Empty for a file that is not test code. */
export function stepSites(text, file) {
  if (SKIP_PATH.test(file)) return [];
  const rust = file.endsWith('.rs');
  if (!rust && !NODE_TEST_FILE.test(file)) return [];
  const masked = rust ? maskRust(text) : maskTs(text);
  let from = 0;
  if (rust && !RUST_TEST_FILE.test(file)) {
    const m = CFG_TEST_MOD.exec(masked);
    if (!m) return [];
    from = m.index;
  }
  const lines = text.split('\n');
  const lineAt = (i) => text.slice(0, i).split('\n').length;
  const out = [];
  const waivedAt = (line) => WAIVER.test(lines[line - 1] ?? '') || WAIVER.test(lines[line - 2] ?? '');
  // A waiver sits on the site's line or the line above, or on the loop's own line or the line above it (`loopAt`).
  const push = (index, rule, what, loopAt = null) => {
    const line = lineAt(index);
    if (waivedAt(line) || (loopAt !== null && waivedAt(lineAt(loopAt)))) return;
    if (!out.some((o) => o.line === line && o.rule === rule)) out.push({ index, line, rule, what });
  };
  const commands = rust && RUST_COMMANDS.test(masked);
  // Node: a sleep from node:timers/promises (imported under any name), or one named sleep/delay, awaited in a loop.
  const timerNames = rust ? [] : [...text.matchAll(/import\s*\{([^}]*)\}\s*from\s*['"](?:node:)?timers\/promises['"]/g)]
    .flatMap((i) => i[1].split(',').map((x) => x.trim().split(/\s+as\s+/).pop()).filter(Boolean));
  const sleeps = rust ? null : new RegExp(`\\bawait\\s+(?:${['sleep', 'delay', ...timerNames].map(escape).join('|')})\\s*\\(`);
  const loops = rust ? /\b(?:for|while|loop)\b/g : /\b(?:for|while)\s*\(/g;
  for (const m of masked.matchAll(loops)) {
    if (m.index < from) continue;
    // `impl Trait for Type {` and `for<'a>` are not loops.
    if (rust && m[0] === 'for' && (/\bimpl\b/.test(masked.slice(statementStart(masked, m.index), m.index)) || /^for\s*</.test(masked.slice(m.index)))) continue;
    const body = bodyAfter(masked, m.index, !rust);
    if (!body) continue;
    const inner = masked.slice(body[0], body[1]);
    let run = null;
    if (rust && commands) {
      for (const r of inner.matchAll(new RegExp(RUST_RUN.source, 'g'))) {
        if (!r[0].includes('status') || commandStatus(masked, body[0] + r.index)) { run = r; break; }
      }
    } else if (!rust) run = NODE_RUN.exec(inner);
    if (run) push(body[0] + run.index, 'A', 'runs a program to completion on every turn of a loop, so the test\'s one time bound covers the sum of the turns', m.index);
    if (!rust) {
      const header = masked.slice(m.index, body[0]);
      for (const t of [header, inner]) {
        const poll = /\bsetTimeout\s*\([^)]*,\s*[^)\s][^)]*\)/.exec(t) ?? (sleeps ? sleeps.exec(t) : null);
        if (poll) push((t === header ? m.index : body[0]) + poll.index, 'B', 'polls at a fixed pace (a timer with a fixed pause inside a loop)');
      }
    }
  }
  if (rust) {
    for (const m of masked.matchAll(/\bspawn_detached\s*\(/g)) {
      if (m.index >= from) push(m.index, 'C', 'starts a detached process (its own process group) that nothing ends when the test fails');
    }
    for (const m of masked.matchAll(/\bfn\s+\w+/g)) {
      if (m.index < from || !commands) continue;
      const body = bodyAfter(masked, m.index);
      if (!body) continue;
      for (const [at, what] of unguardedSpawns(masked.slice(body[0], body[1]), body[0])) push(at, 'C', what);
    }
  } else if (!NODE_GUARD.test(masked)) {
    for (const m of masked.matchAll(/\bdetached\s*:\s*true\b/g)) push(m.index, 'C', 'starts a detached process (its own process group) that nothing in this file reaps');
  }
  return out.sort((a, b) => a.index - b.index || a.rule.localeCompare(b.rule));
}

const FIX = {
  A: 'Run independent cases side by side (`paddock_testkit::run_cases` / `both`), make one test per case, or wait on each step\'s progress (`paddock_testkit::wait_progressing`, `waitProgressing` in scripts/test-kit.mjs).',
  B: 'Wait with `waitProgressing` from scripts/test-kit.mjs: its poll backs off from 1 ms to 100 ms and it fails only when nothing moves.',
  C: 'Start it through `paddock_testkit_start::Reaper` (Rust) or `Reaper` from scripts/test-kit.mjs (Node), which end it and everything it started when the test ends or fails.',
};

export default {
  id: 'C126',
  scope: 'root',
  mode: 'report-only',
  title: 'Tests bound each step on progress and end every process they start',
  rule: 40,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!/\.(?:rs|mjs|js|cjs|ts|mts)$/.test(f)) continue;
      const text = ctx.read(f);
      if (!text || !/for|while|loop|spawn|detached/.test(text)) continue;
      for (const s of stepSites(text, f)) {
        out.push(SEVERITY('C126', 40, f, s.line, `${s.what} (rule ${s.rule}, ADR 0103)`, `${FIX[s.rule]} A site that is fine carries \`// conformance: test-steps — <why>\`.`));
      }
    }
    return out;
  },
};

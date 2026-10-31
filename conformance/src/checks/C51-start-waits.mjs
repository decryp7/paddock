// C51 (report-only warnings; ADR 0031 section 8, widened by the 2026-10-31 amendment): a start or spawn is judged on run state,
// not on a fixed wait. Warns, never fails, about non-test Rust that gives a starting process or service a fixed time to become ready:
// 1. a `Duration` constant, static or struct field whose name says START, SPAWN, LAUNCH or READY and also TIMEOUT, LIMIT, DEADLINE,
//    WAIT, GRACE, BUDGET or WINDOW (`START_LIMIT`, `start_timeout`, `ready_deadline`);
// 2. inside a `fn` whose name starts with start, spawn, launch, boot, connect, handshake, wait_for, ready or await:
//    `Deadline::after(`, `recv_timeout(`, `wait_timeout(` / `wait_timeout_while(`, or `Instant::now() +`;
// 3. in ANY function, a readiness poll by shape: a deadline (`Instant::now() +`, `Deadline::after(`, a `Duration::from_secs(` /
//    `from_millis(` assigned to a name with end, deadline, limit, timeout, until or give_up, or `.elapsed()` compared with `<`/`>`)
//    in a function that loops and, in its body (the `fn` line itself does not count), checks a child's liveness (`try_wait()`,
//    `is_running()`, `is_alive()`, `kill(.., 0)`) or reads readiness (`connect(`, `read_to_string(`, `wait_timeout(`,
//    `recv_timeout(`). An `.elapsed()` comparison counts only with such a signal within 6 lines (a progress throttle does not).
//    Killing the child when the time is up does NOT exempt a function: a run limit must say so with `// cap:`;
// 4. an `Error::Timeout("... start|launch|spawn|boot|ready ...")` string literal, also over several lines or inside `format!`.
// A site passes with `// cap: <why>`, `// external: <why>`, `// shutdown: <why>`, `// ui: <why>`, `// pace: <why>` (a throttle or
// poll interval that bounds nothing) or `// hang-guard: <why>` (a test-only guard against a hang) on the line or the line above, or
// when the line uses `Patience`, or (shape 3) the function uses the shared start judge (`judge_start`, `StartWatch`, `StartJudge`,
// `Patience`; `paddock_proc::StartJudge`). Shape 3 is skipped, by design, in: functions named run*, kill*, close*, terminate*,
// shutdown*, stop*, quit*, wait_closed, wait_event*, wait_timeout, wait_until*, push, flush, resolve or capture_*; any `cdp.rs`
// (request and event waits); `#[test]` functions; and assignments to ready_at, next_* and *_pause. Skipped files: `tests/`,
// `benches/`, `examples/`, `fuzz/`, `tests.rs`, `test.rs`, `testing.rs`, `src/bin/fake_*`, `fixture*` and `test_*`; in-file
// `#[cfg(test)]` / `#[cfg(all(test, ..))]` items are blanked. Scope: `crates/`, `apps/*/crates`, `apps/*/app/src-tauri`, `tools/`.
// The waiver list may only shrink; the check turns to errors when the apps have adopted the judge (ADR 0031 section 8).
import { warn } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

const WAIVER = /\/\/\s*(?:cap|external|shutdown|ui|pace|hang-guard):\s*\S.{2,}/;
const SCOPE = /^(?:crates\/|tools\/|apps\/[^/]+\/crates\/|apps\/[^/]+\/app\/src-tauri\/)/;
const SKIP = /(?:^|\/)(?:tests|benches|examples|fuzz)\/|(?:^|\/)tests?\.rs$|(?:^|\/)testing\.rs$|\/src\/bin\/(?:fake_|fixture|test_)[^/]*$/;
const KIND = '(?:START|SPAWN|LAUNCH|READY)';
const BOUND = '(?:TIMEOUT|LIMIT|DEADLINE|WAIT|GRACE|BUDGET|WINDOW)';
const NAMED = new RegExp(`\\b((?:const|static)\\s+)?([A-Za-z0-9_]*${KIND}[A-Za-z0-9_]*${BOUND}[A-Za-z0-9_]*)\\s*:\\s*(?:Option<\\s*)?(?:std::time::)?Duration\\b`, 'i');
const IN_START_FN = /\b(?:Deadline::after|Instant::now\(\)\s*\+|\.recv_timeout|\.wait_timeout(?:_while)?)\s*\(?/;
const START_FN = /\bfn\s+((?:start|spawn|launch|boot|connect|handshake|wait_for|ready|await)(?:_\w*)?)\b/;
const DEADLINE = /\bInstant::now\(\)\s*\+|\bDeadline::after\s*\(|\b(?:let\s+(?:mut\s+)?)?\w*(?:end|deadline|limit|timeout|until|give_up)\w*\s*(?::[^=]+)?=[^;]*\bDuration::from_(?:secs|millis|secs_f64)\s*\(|\.elapsed\(\)\s*(?:>=?|<=?)/i;
const LIVENESS = /\.(?:try_wait|is_running|is_alive)\s*\(|\bkill\s*\([^)]*,\s*0\s*\)/;
const READINESS = /\bconnect\s*\(|\bread_to_string\s*\(|\.(?:wait_timeout(?:_while)?|recv_timeout)\s*\(/;
// Functions that stop, kill, close or run something: a grace or run limit there is a cap, not a start wait. CDP event waits likewise.
const NOT_A_START = /^(?:(?:run|kill|close|terminate|shutdown|stop|quit)(?:_\w*)?|wait_closed|wait_event\w*|wait_timeout|wait_until\w*|push|flush|resolve|capture_\w+)$/;
const CDP_FILE = /(?:^|\/)cdp(?:\/|\.rs$)/;
const ELAPSED_ONLY = /\.elapsed\(\)\s*(?:>=?|<=?)/;
const NOT_A_DEADLINE_NAME = /\b(?:ready_at|next_\w+|\w+_pause)\s*[:=]/;
const JUDGE = /\b(?:judge_start|StartWatch|StartJudge|Patience)\b/;
const LOOP = /\b(?:loop|while|for)\b/;
const TIMEOUT_LITERAL = /\bTimeout\s*\(\s*(?:format!\s*\(\s*)?"[^"]*\b(?:start|launch|spawn|boot|ready)\w*[^"]*"/gi;

const lineAt = (text, index) => text.slice(0, index).split('\n').length;

/** `text` with the items matching `head` (an attribute plus the item's opening brace) blanked up to their closing brace, newlines kept. */
function blankItems(text, head) {
  const masked = maskRust(text);
  let out = text;
  for (const m of masked.matchAll(head)) {
    let depth = 0;
    let end = masked.length;
    for (let i = m.index + m[0].length - 1; i < masked.length; i++) {
      if (masked[i] === '{') depth++;
      else if (masked[i] === '}' && --depth === 0) {
        end = i + 1;
        break;
      }
    }
    out = out.slice(0, m.index) + out.slice(m.index, end).replace(/[^\n]/g, ' ') + out.slice(end);
  }
  return out;
}

const TEST_ATTR = String.raw`#\[(?:cfg\((?:all\(\s*)?test\b[^\]]*\)|(?:\w+::)?test\b[^\]]*)\]`;
const TEST_ITEM = new RegExp(String.raw`${TEST_ATTR}\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:mod|fn)\s+\w+[^{;]*\{`, 'g');

/** `text` with every test item (`#[cfg(test)]` / `#[cfg(all(test, ..))]` modules and functions, `#[test]` functions) blanked, so code after it still counts. */
const withoutTests = (text) => blankItems(text, TEST_ITEM);

/** The fixed start waits in one Rust source: [{ line, what }] for the sites that carry no waiver. */
export function startWaits(source, path = '') {
  const text = withoutTests(source);
  const masked = maskRust(text);
  const lines = text.split('\n');
  const waived = (line) => {
    const near = `${lines[line - 1] ?? ''}\n${lines[line - 2] ?? ''}`;
    return WAIVER.test(near) || /\bPatience\b/.test(lines[line - 1] ?? '');
  };
  const out = [];
  masked.split('\n').forEach((l, i) => {
    const m = NAMED.exec(l);
    if (m && !waived(i + 1)) out.push({ line: i + 1, what: `\`${m[2]}\` is a fixed time for a start` });
  });
  const found = new Map();
  const add = (line, what) => found.has(line) || found.set(line, what);
  for (const [line, what] of out.map((o) => [o.line, o.what])) add(line, what);
  // Functions, by brace matching on the masked text: from the `fn` line to the line of its closing brace.
  const ml = masked.split('\n');
  const fns = [];
  for (const m of masked.matchAll(/\bfn\s+(\w+)/g)) {
    let i = m.index + m[0].length;
    let paren = 0;
    while (i < masked.length && !(paren === 0 && (masked[i] === '{' || masked[i] === ';'))) {
      if (masked[i] === '(') paren++;
      else if (masked[i] === ')') paren--;
      i++;
    }
    if (masked[i] !== '{') continue;
    let depth = 0;
    let j = i;
    for (; j < masked.length; j++) {
      if (masked[j] === '{') depth++;
      else if (masked[j] === '}' && --depth === 0) break;
    }
    fns.push({ name: m[1], start: lineAt(masked, m.index) - 1, end: lineAt(masked, j) - 1 });
  }
  fns.sort((x, y) => y.start - x.start); // innermost first, so a nested fn names its own lines
  const bodyOf = (f) => {
    const body = ml.slice(f.start, f.end + 1);
    // The `fn` line keeps only what follows its opening brace; a nested fn is blanked while its outer fn is scanned.
    const sig = body[0];
    body[0] = sig.includes('{') ? sig.slice(sig.indexOf('{') + 1) : '';
    for (const g of fns) {
      if (g !== f && g.start >= f.start && g.end <= f.end && (g.start > f.start || g.end < f.end)) {
        for (let k = Math.max(g.start, f.start + (g.start === f.start ? 1 : 0)); k <= g.end; k++) body[k - f.start] = '';
      }
    }
    return body;
  };
  for (const f of fns) {
    const body = bodyOf(f);
    if (START_FN.test(ml[f.start])) {
      body.forEach((l, k) => {
        const i = f.start + k;
        if (IN_START_FN.test(l) && !waived(i + 1)) add(i + 1, `a fixed wait inside \`${f.name}\``);
      });
    }
    if (NOT_A_START.test(f.name) || CDP_FILE.test(path)) continue;
    const joined = body.join('\n');
    if (JUDGE.test(joined) || !LOOP.test(joined)) continue;
    if (!(LIVENESS.test(joined) || READINESS.test(joined))) continue;
    body.forEach((l, k) => {
      const i = f.start + k;
      const nearby = body.slice(Math.max(0, k - 6), k + 7).join('\n');
      const elapsedOnly = ELAPSED_ONLY.test(l) && !/Instant::now\(\)\s*\+|Deadline::after/.test(l);
      if (elapsedOnly && !(LIVENESS.test(nearby) || READINESS.test(nearby))) return;
      if (DEADLINE.test(l) && !NOT_A_DEADLINE_NAME.test(l) && !waived(i + 1)) add(i + 1, `a readiness poll on a fixed deadline in \`${f.name}\``);
    });
  }
  for (const m of text.matchAll(TIMEOUT_LITERAL)) {
    if (!masked.startsWith('Timeout', m.index)) continue; // inside a comment or string
    const line = lineAt(text, m.index);
    if (!waived(line)) add(line, 'a timeout error that names a start');
  }
  out.length = 0;
  for (const [line, what] of found) out.push({ line, what });
  return out.sort((a, b) => a.line - b.line);
}

export default {
  id: 'C51',
  title: 'Starts are judged on run state, not on a fixed wait (report-only)',
  rule: 40,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs') || !SCOPE.test(f) || SKIP.test(f)) continue;
      const text = ctx.read(f);
      if (!text) continue;
      for (const s of startWaits(text, f)) {
        out.push(warn('C51', 40, f, s.line, `${s.what}: a working start must not fail only because time passed (ADR 0031, ER-19)`, 'Judge it with `paddock_proc::StartJudge` / `StartWatch`, or mark a real cap with `// cap: <why>` (also `external`, `shutdown`, `ui`, `pace`).'));
      }
    }
    return out;
  },
};

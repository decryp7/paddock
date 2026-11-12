// C71: tests do not wait on real time as a settle (ER-12, rule 5, rule 49, ADR 0041 §4). Opt-in with `"timingLint": true` in
// paddock.conformance.json (the Paddock repo turns it on and scans every app below it). Flags, in test code only:
//   TypeScript specs, tests and e2e files: `waitForTimeout(`; a `setTimeout` used as a wait (`await new Promise(r => setTimeout(r, n))`,
//     `await sleep(n)` / `delay(n)`); a wait on `requestAnimationFrame` turns; and a locator count read once and used later to index
//     (`const n = await rows.count()` ... `rows.nth(n - 1)` or a `for (i < n)` loop over `.nth(i)`): the count can change between the
//     two calls (lesson "A sidebar spec counted the links once and indexed past the end"); use `.last()`, `.first()` or an assertion
//     on the count instead.
//   Rust test code (`tests/`, `benches/`, `tests.rs`, `*_tests.rs`, and the `#[cfg(test)]` items): `thread::sleep` and
//     `tokio::time::sleep` (a settle: wait on the event, a channel or the FakeClock).
// Exemptions are per call (ADR 0041 §4): `// conformance: hang-guard — <why>` on the line or the line above, and the reason names a
// bound of at least 10 s (a generous limit that only stops a hung test; `real-wait` is not an exemption); a TypeScript call under
// `useTestClock()` / `frozenClock(` (installed in an enclosing scope before the call); a Rust call that takes a `FakeClock`,
// `ManualPace` or `TestSeams` as an argument. A file that merely mentions a fake clock exempts nothing. `paddock-testkit` owns real
// time and is skipped.
// Existing sites sit in `conformance/baselines/C71.json` as {file, hash of the normalised line}: a site in the baseline passes, any
// other is an error. The baseline may only shrink: a listed site that is gone is a warning to remove it, and an entry that the
// baseline at the merge base (`origin/main`, or PADDOCK_BASE_REF) does not have is an error. There is no expiry: ADR 0041 WP-6 is open-ended.
// Fixed budgets (ADR 0071 section 4, R2 and R3 in their smallest form; own baseline `conformance/baselines/C71-waits.json`, shrink-only
// the same way): in Rust test code, a `Duration` literal under 40 s given to a wait helper (`wait_until`, `wait_for`, `poll_for`,
// `wait_ready_or_exit`, `wait_exit`, `recv_timeout`, `wait_timeout`) or assigned to a start setting (`connect_window`, `ready_timeout`,
// `start_deadline`, ...). Wait for the condition (ready or exit), or give the kit's `HANG_GUARD`/`UNTIL_CONNECTED`; a 40 s or longer
// literal is a hang guard. Exempt: a `FakeClock`/`ManualPace`/`TestSeams` argument, or `// conformance: expiry-under-test — <why>`
// (the test examines that window). No hang-guard waiver: a 20 s guard on a start is the bug.
// In TypeScript specs: `vi.waitFor`, `waitFor`, `waitUntil`, `expect.poll` or `.toPass` with `{ timeout: n }` under 40 000 ms.
// State races (ER-21, own baseline `conformance/baselines/C71-races.json`): a test that asserts on state another thread writes, right
// after waiting on a different condition (Debrief's ticker test, CI 7963; Dyno's ghost test, lessons 2026-11-01). Rust: the statement
// after `wait_until(..)`/`wait_for(..)` is an `assert*!` that reads a field (outside call arguments) the wait's condition did not read;
// an assertion that the field is still `None`/`is_none()` is an ordering check and passes. TypeScript: `{ ...x.status() }` (or
// `snapshot`/`state`/`current`) in the statement right after an `await` of a timer or settle (`advanceTimers*`, `settle`, `flush*`,
// `tick`) that did not read that getter: the copy is whichever state the race left. Wait for the field itself, or build the value from
// scratch. Not a race: an assertion on the name the wait's result is bound to, a field the wait's receiver names, or a site with
// `// conformance: ordered — <why>` (on the wait or the assertion, or the line above either).
// Also (ER-12): a Playwright config (`playwright*.config.*`) whose `retries` can be above 0 sets `failOnFlakyTests` (not `false`), so a
// retry never turns a red run green; a config with `retries: 0` or no `retries` is fine.
// Not yet (ADR 0041 §4 amendment, "first slice"): Instant/SystemTime/Date.now, Deadline::after,
// performance.now, page.clock.install without pauseAt, vi.waitFor under fake timers, the live-status spread, C30's elapsed asserts,
// vm-smoke `observe()`, the ESLint rule.
import { createHash } from 'node:crypto';
import { baselineGrowth as sharedGrowth } from '../util/baseline.mjs';
import { error, warn } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

export const BASELINE_PATH = 'conformance/baselines/C71.json';
export const WAITS_BASELINE_PATH = 'conformance/baselines/C71-waits.json';
export const RACES_BASELINE_PATH = 'conformance/baselines/C71-races.json';
const WAIVER = /conformance:\s*hang-guard\s*[—–-]+\s*(\S.{2,})/;
const BOUND = /(\d+(?:\.\d+)?)\s*(ms|milliseconds?|min|mins|minutes?|s|secs?|seconds?)\b/g;
const FAKE_SEAM = /\b(?:FakeClock|ManualPace|TestSeams)\b/;
const TS_CLOCK = /\b(?:useTestClock|frozenClock)\s*\(/;
const TS_TEST = /(?:\.(?:spec|test)\.(?:ts|mts|cts|js|mjs|cjs)$|(?:^|\/)(?:e2e|selftest)\/.+\.(?:ts|mjs|js)$)/;
const SKIP_PATH = /^conformance\/test\/|(?:^|\/)(?:node_modules|dist|target|fixtures)\/|^apps\/[^/]+\/vendor\/|(?:^|\/)crates\/paddock-testkit(?:-start)?\//;
const RUST_TEST_FILE = /(?:^|\/)(?:tests|benches)\/.+\.rs$|(?:^|\/)tests\.rs$|_tests?\.rs$/;
const SLEEP_FN = /\b(?:std::)?thread::sleep\s*\(|\b(?:tokio::)?time::sleep\s*\(/;

/** Hash of a line's text with whitespace collapsed, so a site is known by what it says and survives moving. */
export const lineHash = (text) => createHash('sha256').update(text.replace(/\s+/g, ' ').trim()).digest('hex').slice(0, 16);

/** The longest bound a waiver reason names, in seconds (0 when it names none): "a 20 s bound", "90000 ms", "2 min". */
export function boundSeconds(reason) {
  let best = 0;
  for (const m of reason.matchAll(BOUND)) {
    const n = Number(m[1]);
    const unit = m[2][0] === 'm' ? (m[2].startsWith('mi') ? 60 : 0.001) : 1;
    best = Math.max(best, n * unit);
  }
  return best;
}

const waiverOk = (line) => {
  const m = WAIVER.exec(line ?? '');
  return m !== null && boundSeconds(m[1]) >= 10;
};
const waived = (lines, i) => waiverOk(lines[i]) || waiverOk(lines[i - 1]);

/**
 * `text` with comments and the contents of strings, template text and regular expressions blanked (same length, newlines kept), so
 * commented-out code, a string that holds code and a `//` inside a string are not read as code. Code inside a template `${...}` stays.
 */
export function maskTs(text) {
  const out = text.split('');
  const blank = (a, b) => {
    for (let k = a; k < b && k < out.length; k++) if (out[k] !== '\n') out[k] = ' ';
  };
  let i = 0;
  const template = () => {
    let start = i;
    while (i < text.length) {
      const c = text[i];
      if (c === '\\') i += 2;
      else if (c === '`') {
        blank(start, i);
        i++;
        return;
      } else if (c === '$' && text[i + 1] === '{') {
        blank(start, i);
        i += 2;
        code(true);
        start = i;
      } else i++;
    }
    blank(start, text.length);
  };
  const code = (inSubstitution) => {
    let depth = 0;
    let prev = '';
    while (i < text.length) {
      const c = text[i];
      const n = text[i + 1];
      if (c === '/' && n === '/') {
        const e = text.indexOf('\n', i);
        const end = e < 0 ? text.length : e;
        blank(i, end);
        i = end;
        continue;
      }
      if (c === '/' && n === '*') {
        const e = text.indexOf('*/', i + 2);
        const end = e < 0 ? text.length : e + 2;
        blank(i, end);
        i = end;
        continue;
      }
      if (c === '"' || c === "'") {
        let j = i + 1;
        while (j < text.length && text[j] !== c && text[j] !== '\n') j += text[j] === '\\' ? 2 : 1;
        blank(i + 1, j);
        i = j + 1;
        prev = c;
        continue;
      }
      if (c === '`') {
        i++;
        template();
        prev = '`';
        continue;
      }
      if (c === '/' && (prev === '' || '(,=:[!&|?{};'.includes(prev))) {
        let j = i + 1;
        let inClass = false;
        while (j < text.length && text[j] !== '\n') {
          if (text[j] === '\\') j += 2;
          else if (text[j] === '[') inClass = true;
          else if (text[j] === ']') inClass = false;
          else if (text[j] === '/' && !inClass) break;
          if (text[j] !== '\\') j++;
        }
        blank(i + 1, j);
        i = j + 1;
        prev = '/';
        continue;
      }
      if (c === '{') depth++;
      else if (c === '}') {
        if (inSubstitution && depth === 0) {
          i++;
          return;
        }
        depth--;
      }
      if (!/\s/.test(c)) prev = c;
      i++;
    }
  };
  code(false);
  return out.join('');
}

/**
 * Whether a fake clock is installed by a statement of a scope that encloses `offset`, before it. Each open scope contributes its own
 * text only: a clock installed in a block that already closed (a sibling test) does not count. The file is the outermost scope.
 */
function clockInstalled(code, offset) {
  const scopes = [[]];
  let from = 0;
  for (let k = 0; k < offset; k++) {
    if (code[k] !== '{' && code[k] !== '}') continue;
    scopes.at(-1).push(code.slice(from, k));
    from = k + 1;
    if (code[k] === '{') scopes.push([]);
    else if (scopes.length > 1) scopes.pop();
  }
  scopes.at(-1).push(code.slice(from, offset));
  return scopes.some((parts) => TS_CLOCK.test(parts.join(' ')));
}

/** The timing sites of one TypeScript test file: [{ line, what }] without waivers. */
export function tsTimingSites(source) {
  const lines = source.split('\n');
  const masked = maskTs(source);
  const code = masked.split('\n');
  const starts = [];
  code.reduce((acc, l) => (starts.push(acc), acc + l.length + 1), 0);
  const out = [];
  const add = (i, what) => {
    if (!waived(lines, i) && !clockInstalled(masked, starts[i])) out.push({ line: i + 1, what });
  };
  const counts = []; // { name, at } for `const n = await x.count()`
  code.forEach((l, i) => {
    if (/\bwaitForTimeout\s*\(/.test(l)) add(i, '`waitForTimeout` is a fixed wait');
    else if (/\bsetTimeout\s*\(/.test(l) && /\b(?:await|Promise|resolve)\b/.test(l)) add(i, '`setTimeout` used as a wait');
    else if (/\bawait\s+(?:sleep|delay|wait)\s*\(\s*\d/.test(l)) add(i, 'a fixed `sleep`/`delay` wait');
    else if (/\brequestAnimationFrame\s*\(/.test(l)) add(i, 'waiting on `requestAnimationFrame` turns');
    const c = /\b(?:const|let|var)\s+(\w+)\s*=\s*await\s+[^;]*\.count\(\)/.exec(l);
    if (c) counts.push({ name: c[1], at: i });
    const nth = /\.nth\(([^)]*)\)/.exec(l);
    if (nth) {
      for (const { name, at } of counts) {
        if (i <= at || i - at > 60) continue;
        const uses = new RegExp(`\\b${name}\\b`).test(nth[1]);
        const viaLoop = /^\s*\w+\s*$/.test(nth[1]) && code.slice(at, i).some((x) => new RegExp(`\\bfor\\b[^;]*;[^;]*[<]=?\\s*${name}\\b`).test(x));
        if (uses || viaLoop) {
          add(i, `\`.nth()\` indexed with the count read at line ${at + 1}: the count can change in between (use \`.last()\`, \`.first()\` or assert the count)`);
          break;
        }
      }
    }
  });
  return out;
}

/** Character ranges of the `#[cfg(test)]` items of masked Rust source: [start, end) from the attribute to the item's closing brace. */
function rustTestRanges(masked) {
  const ranges = [];
  for (const m of masked.matchAll(/#\[cfg\(test\)\]/g)) {
    let k = m.index + m[0].length;
    for (;;) {
      const rest = /^\s*#\[/.exec(masked.slice(k, k + 40));
      if (!rest) break;
      const close = masked.indexOf(']', k + rest[0].length);
      if (close < 0) break;
      k = close + 1;
    }
    while (k < masked.length && masked[k] !== '{' && masked[k] !== ';') k++;
    if (masked[k] !== '{') continue; // `#[cfg(test)] use ...;` or `mod tests;` is no region
    let depth = 0;
    let e = k;
    for (; e < masked.length; e++) {
      if (masked[e] === '{') depth++;
      else if (masked[e] === '}' && --depth === 0) break;
    }
    ranges.push([m.index, e + 1]);
  }
  return ranges;
}

/** The text of the parenthesised argument list that opens at `open` (masked source), or '' when it does not close. */
function callArgs(masked, open) {
  let depth = 0;
  for (let k = open; k < masked.length; k++) {
    if (masked[k] === '(') depth++;
    else if (masked[k] === ')' && --depth === 0) return masked.slice(open + 1, k);
  }
  return '';
}

/** The sleeps in the test code of one Rust file: [{ line, what }] without waivers. */
export function rustTimingSites(source, { wholeFileIsTest = false } = {}) {
  const lines = source.split('\n');
  const masked = maskRust(source);
  const ranges = wholeFileIsTest ? [[0, masked.length]] : rustTestRanges(masked);
  if (!ranges.length) return [];
  const bareSleep = /\buse\s+(?:std::thread|tokio::time)::(?:\{[^}]*\bsleep\b[^}]*\}|sleep)\b/.test(masked);
  const out = [];
  let offset = 0;
  masked.split('\n').forEach((line, i) => {
    const at = offset;
    offset += line.length + 1;
    if (!ranges.some(([a, b]) => at >= a && at < b)) return;
    const m = SLEEP_FN.exec(line) ?? (bareSleep ? /(?<![:\w.])sleep\s*\(/.exec(line) : null);
    if (!m) return;
    if (waived(lines, i) || FAKE_SEAM.test(callArgs(masked, at + m.index + m[0].length - 1))) return;
    out.push({ line: i + 1, what: 'a sleep in a test is a settle: wait on the event, a channel or the FakeClock' });
  });
  return out;
}

const WAIT_HELPER = /\b(?:wait_until|wait_for|poll_for|wait_ready_or_exit|wait_exit|recv_timeout|wait_timeout)\s*\(/;
const START_SETTING = /\b(?:\w*(?:connect|hello|start|ready|spawn|launch|load)\w*_(?:window|timeout|deadline|limit|budget)|ready|start)\s*[:=](?!=)/;
const DURATION_LITERAL = /\bDuration::from_(secs|millis|micros|nanos|secs_f32|secs_f64)\s*\(\s*([\d_]+(?:\.\d+)?)\s*\)/g;
const EXPIRY_WAIVER = /conformance:\s*expiry-under-test\s*[—–-]+\s*\S.{2,}/;
/** A literal this small is a settle, not a hang guard. */
const GUARD_SECONDS = 40;

/** Seconds of the smallest `Duration` literal in `text` (Infinity when there is none). */
function smallestLiteral(text) {
  let best = Infinity;
  for (const m of text.matchAll(DURATION_LITERAL)) {
    const n = Number(m[2].replace(/_/g, ''));
    const scale = { secs: 1, secs_f32: 1, secs_f64: 1, millis: 1e-3, micros: 1e-6, nanos: 1e-9 }[m[1]];
    best = Math.min(best, n * scale);
  }
  return best;
}

/** The fixed budgets in the test code of one Rust file (see the header): [{ line, what }] without waivers. */
export function rustWaitSites(source, { wholeFileIsTest = false } = {}) {
  const lines = source.split('\n');
  const masked = maskRust(source);
  const ranges = wholeFileIsTest ? [[0, masked.length]] : rustTestRanges(masked);
  if (!ranges.length) return [];
  const out = [];
  let offset = 0;
  masked.split('\n').forEach((line, i) => {
    const at = offset;
    offset += line.length + 1;
    if (!ranges.some(([a, b]) => at >= a && at < b)) return;
    const call = WAIT_HELPER.exec(line);
    let region = '';
    if (call) region = callArgs(masked, at + call.index + call[0].length - 1);
    else if (START_SETTING.test(line)) region = line;
    if (!region || smallestLiteral(region) >= GUARD_SECONDS) return;
    if (FAKE_SEAM.test(region) || EXPIRY_WAIVER.test(lines[i]) || EXPIRY_WAIVER.test(lines[i - 1] ?? '')) return;
    out.push({ line: i + 1, what: 'a fixed Duration under 40 s given to a wait or a start setting: the host sets how long the thing needs' });
  });
  return out;
}

const RACE_WAIT = /\b(?:wait_until|wait_for|poll_for)\s*\(/;
const FIELD = /\.([A-Za-z_]\w*)\b(?!\s*\(|::)/g; // a method call `.f(` and a turbofish `.collect::<..>()` are not fields
const ORDERED_WAIVER = /conformance:\s*ordered\s*[—–-]+\s*\S.{2,}/;

/** Field names read in `text`, optionally leaving out what sits inside call arguments (`info(h.id)` reads no `id` of the subject). */
function fieldsOf(text, { outsideCalls }) {
  let t = text;
  if (outsideCalls) for (let prev = ''; prev !== t; ) { prev = t; t = t.replace(/(?<=\w)\([^()]*\)/g, '()'); }
  return new Set([...t.matchAll(FIELD)].map((m) => m[1]));
}

/** The statement races in the test code of one Rust file (see the header): [{ line, what }]. */
export function rustRaceSites(source, { wholeFileIsTest = false } = {}) {
  const lines = source.split('\n');
  const masked = maskRust(source);
  const ranges = wholeFileIsTest ? [[0, masked.length]] : rustTestRanges(masked);
  if (!ranges.length) return [];
  const out = [];
  const lineOf = (k) => masked.slice(0, k).split('\n').length;
  for (const m of masked.matchAll(new RegExp(RACE_WAIT.source, 'g'))) {
    if (!ranges.some(([a, b]) => m.index >= a && m.index < b)) continue;
    const open = m.index + m[0].length - 1;
    const args = callArgs(masked, open);
    const end = masked.indexOf(';', open + args.length + 1);
    if (end < 0) continue;
    const stmtEnd = masked.indexOf(';', end + 1);
    if (stmtEnd < 0) continue;
    const stmt = masked.slice(end + 1, stmtEnd);
    if (!/^\s*assert(?:_eq|_ne)?!\s*\(/.test(stmt) || /\bNone\b|\bis_none\s*\(/.test(stmt)) continue;
    // The wait's own statement: what it is called on (`r.renderers.wait_for(..)` reads `renderers`) and the name it is bound to
    // (`let seen = c.wait_for(..)`): an assertion on that name reads what the wait returned, not state a thread may still write.
    const stmtStart = Math.max(masked.lastIndexOf(';', m.index), masked.lastIndexOf('{', m.index), masked.lastIndexOf('}', m.index)) + 1;
    const head = masked.slice(stmtStart, m.index);
    const read = new Set([...fieldsOf(args, { outsideCalls: false }), ...fieldsOf(head, { outsideCalls: false })]);
    const bound = /\blet\s+(?:mut\s+)?(\w+)\s*(?::[^=]+)?=/.exec(head)?.[1];
    if (bound && new RegExp(`\\b${bound}\\b`).test(stmt)) continue;
    if (![...fieldsOf(stmt, { outsideCalls: true })].some((f) => !read.has(f))) continue;
    const at = lineOf(end + 1 + stmt.search(/\S/));
    const ordered = [lineOf(m.index) - 1, lineOf(m.index), at - 1].some((n) => ORDERED_WAIVER.test(lines[n - 1] ?? '')) || ORDERED_WAIVER.test(lines[at - 2] ?? '');
    if (ordered) continue;
    out.push({ line: at, what: 'an assertion on a field the wait before it did not read: the writer may not have written it yet' });
  }
  return out;
}

const TS_SPREAD = /\.\.\.\s*([\w.$!?]+)\.(?:status|snapshot|state|current)\s*\(\s*\)/;
const TS_TIMER_WAIT = /\bawait\s+(?:vi\.advanceTimers\w*|vi\.runAllTimers\w*|vi\.runOnlyPendingTimers\w*|settle|flush\w*|tick)\s*\(/;

/** The current-status copies after a timer or settle wait in one TypeScript test file: [{ line, what }] (see the header). */
export function tsRaceSites(source) {
  const masked = maskTs(source);
  const lines = masked.split('\n');
  const out = [];
  lines.forEach((line, i) => {
    const sp = TS_SPREAD.exec(line);
    if (!sp) return;
    // The previous statement: the nearest earlier non-blank line that ends a statement (`;` or `});`), read back to its start.
    let j = i - 1;
    while (j >= 0 && lines[j].trim() === '') j--;
    if (j < 0) return;
    let k = j;
    while (k > 0 && !/[;{]\s*$/.test(lines[k - 1].trimEnd()) ) k--;
    const prev = lines.slice(k, j + 1).join('\n');
    if (!TS_TIMER_WAIT.test(prev) || prev.includes(`${sp[1]}.`) ) return;
    out.push({ line: i + 1, what: 'a copy of the current status right after a timer or settle wait: it is whichever state the race left' });
  });
  return out;
}

const TS_WAIT = /\b(?:vi\.waitFor|waitFor|waitUntil|expect\.poll|toPass)\s*\(/;
const TS_TIMEOUT = /\btimeout\s*:\s*([\d_]+)\b/;

/**
 * The fixed budgets in one TypeScript test file: `vi.waitFor`, `waitFor`, `waitUntil`, `expect.poll` or `.toPass` called with
 * `{ timeout: n }` of less than 40 000 ms (a test's own `{ timeout }` option is the runner's hang guard, not a wait, and is not read).
 * [{ line, what }]; the call's first line. Exempt under `useTestClock()` / `frozenClock(` or with `// conformance: expiry-under-test — <why>`.
 */
export function tsWaitSites(source) {
  const lines = source.split('\n');
  const masked = maskTs(source);
  const out = [];
  let offset = 0;
  masked.split('\n').forEach((line, i) => {
    const at = offset;
    offset += line.length + 1;
    const call = TS_WAIT.exec(line);
    if (!call) return;
    const args = callArgs(masked, at + call.index + call[0].length - 1);
    const t = TS_TIMEOUT.exec(args);
    if (!t || Number(t[1].replace(/_/g, '')) >= GUARD_SECONDS * 1000) return;
    if (EXPIRY_WAIVER.test(lines[i]) || EXPIRY_WAIVER.test(lines[i - 1] ?? '') || clockInstalled(masked, at)) return;
    out.push({ line: i + 1, what: 'a fixed `timeout` under 40 s given to a wait: the host sets how long the thing needs' });
  });
  return out;
}

/** Every fixed budget in a file tree, in the shape of [`timingSites`]. */
export function waitSites(files, read, finders = { ts: tsWaitSites, rust: rustWaitSites }) {
  const out = [];
  for (const file of files) {
    if (SKIP_PATH.test(file)) continue;
    const isTs = TS_TEST.test(file);
    if (!isTs && !file.endsWith('.rs')) continue;
    const text = read(file);
    if (!text) continue;
    const found = isTs ? finders.ts(text) : finders.rust(text, { wholeFileIsTest: RUST_TEST_FILE.test(file) });
    const lines = text.split('\n');
    for (const s of found) out.push({ file, ...s, text: lines[s.line - 1], hash: lineHash(lines[s.line - 1]) });
  }
  return out;
}

/** Every state race in a file tree, in the shape of [`timingSites`]. */
export const raceSites = (files, read) => waitSites(files, read, { ts: tsRaceSites, rust: rustRaceSites });

/** Every site in a file tree: [{ file, line, what, hash, text }]. */
export function timingSites(files, read) {
  const out = [];
  for (const file of files) {
    if (SKIP_PATH.test(file)) continue;
    let found;
    if (TS_TEST.test(file)) {
      const text = read(file);
      if (text) found = tsTimingSites(text);
    } else if (file.endsWith('.rs')) {
      const text = read(file);
      if (text) found = rustTimingSites(text, { wholeFileIsTest: RUST_TEST_FILE.test(file) });
    }
    if (!found?.length) continue;
    const lines = read(file).split('\n');
    for (const s of found) out.push({ file, ...s, text: lines[s.line - 1], hash: lineHash(lines[s.line - 1]) });
  }
  return out;
}

/** Splits `sites` against the baseline entries: { fresh, known, stale } (a hash is used once per entry, so a copy is fresh). */
export function againstBaseline(sites, entries) {
  const left = new Map();
  for (const e of entries) left.set(`${e.file}\0${e.hash}`, (left.get(`${e.file}\0${e.hash}`) ?? 0) + 1);
  const fresh = [];
  for (const s of sites) {
    const k = `${s.file}\0${s.hash}`;
    if ((left.get(k) ?? 0) > 0) left.set(k, left.get(k) - 1);
    else fresh.push(s);
  }
  const stale = entries.filter((e) => (left.get(`${e.file}\0${e.hash}`) ?? 0) > 0);
  return { fresh, stale };
}

const key = (e) => `${e.file}\0${e.hash}`;

/** The baseline entries that still match a site, with the sites' current lines: the baseline after removing the stale ones, never adding. */
export function shrinkBaseline(sites, entries) {
  const left = new Map();
  for (const e of entries) left.set(key(e), (left.get(key(e)) ?? 0) + 1);
  const kept = [];
  for (const s of sites) {
    if ((left.get(key(s)) ?? 0) > 0) {
      left.set(key(s), left.get(key(s)) - 1);
      kept.push({ file: s.file, line: s.line, hash: s.hash });
    }
  }
  return kept;
}

/**
 * The baseline entries that the baseline at the merge base does not have (a multiset difference by file and hash), or `{ reason }`
 * when it could not be measured. One shrink-only rule for every baseline file: conformance/src/util/baseline.mjs.
 */
export function baselineGrowth(ctx, current, path = BASELINE_PATH) {
  return sharedGrowth({
    root: ctx.root,
    path,
    current,
    keyOf: key,
    entriesOf: (json) => json.sites ?? [],
    ...(ctx.options.git ? { git: ctx.options.git } : {}),
    ...(ctx.options.baseRef ?? process.env.PADDOCK_BASE_REF ? { baseRef: ctx.options.baseRef ?? process.env.PADDOCK_BASE_REF } : {}),
  });
}

const PW_CONFIG = /(?:^|\/)playwright[^/]*\.config\.(?:ts|mts|cts|js|mjs|cjs)$/;

/** The 1-based line of a `retries` setting that can exceed 0 without `failOnFlakyTests` (ER-12), or 0 when the config is fine. */
export function retriesWithoutFlakyFailure(text) {
  const code = maskTs(text);
  const m = /\bretries\s*:(?!\s*0\s*(?:[,}\n]|$))/.exec(code);
  if (!m) return 0;
  if (/\bfailOnFlakyTests\s*:\s*(?!false\b)\S/.test(code)) return 0;
  return code.slice(0, m.index).split('\n').length;
}

export default {
  id: 'C71',
  title: 'Tests do not settle on real time',
  rule: 49,
  scope: 'both',
  mode: 'gate',
  run(ctx) {
    if (!ctx.config.timingLint) return [];
    const flaky = ctx.files().filter((f) => PW_CONFIG.test(f) && !SKIP_PATH.test(f)).flatMap((f) => {
      const line = retriesWithoutFlakyFailure(ctx.read(f) ?? '');
      return line ? [error('C71', 49, f, line, '`retries` can be above 0 without `failOnFlakyTests` (ER-12): a flaky test would pass its job unseen', 'Add `failOnFlakyTests: true` next to `retries` (Playwright 1.50+), so a test that needed its retry fails the run.')] : [];
    });
    const readBaseline = (path) => {
      const raw = ctx.read(path);
      return raw ? JSON.parse(raw) : { sites: [] };
    };
    const fixFor = {
      [BASELINE_PATH]: 'Wait on the condition (an event, `expect.poll`, a locator assertion, `useTestClock`/`frozenClock`, the FakeClock), or for a real bound add `// conformance: hang-guard — <why>, <bound of at least 10 s>` on the line.',
      [RACES_BASELINE_PATH]: 'Wait for the field the assertion reads (a second `wait_until` on it), or force the order the test needs; build a pushed status from scratch instead of copying the current one (lessons: "A test that copies the current status inherits whichever state the race left").',
      [WAITS_BASELINE_PATH]: 'Wait for the condition, ready or exit (ADR 0071: `wait_ready_or_exit`, `wait_until`), or pass the kit\'s `HANG_GUARD`/`UNTIL_CONNECTED`; a fixed budget under 40 s is a verdict the host speed decides. If the test examines that window, add `// conformance: expiry-under-test — <why>`.',
    };
    const out = [...flaky];
    for (const [path, sites] of [[BASELINE_PATH, timingSites(ctx.files(), ctx.read)], [WAITS_BASELINE_PATH, waitSites(ctx.files(), ctx.read)], [RACES_BASELINE_PATH, raceSites(ctx.files(), ctx.read)]]) {
      const baseline = readBaseline(path);
      const { fresh, stale } = againstBaseline(sites, baseline.sites);
      out.push(...fresh.map((s) => error('C71', 49, s.file, s.line, `${s.what} (ER-12, rule 49): ${s.text.trim().slice(0, 100)}`, fixFor[path])));
      for (const e of stale) {
        out.push(warn('C71', 49, e.file, e.line ?? 1, `${path} lists a site that is gone (hash ${e.hash}); the baseline may only shrink`, 'Remove the entry (`node conformance/bin/paddock-timing-baseline.mjs --shrink` rewrites the file without it; add `--waits` for the fixed budgets).'));
      }
      if (baseline.sites.length) {
        const g = baselineGrowth(ctx, baseline.sites, path);
        if (g.reason) {
          out.push(warn('C71', 49, path, 1, `the baseline's shrink-only rule is not enforced here: ${g.reason}`, 'Fetch the target branch (`git fetch origin main`, full history), or set PADDOCK_BASE_REF to the ref to compare with.'));
        } else if (g.added.length) {
          const list = g.added.slice(0, 8).map((e) => `${e.file}:${e.line ?? '?'}`).join(', ');
          out.push(error('C71', 49, path, 1, `the baseline grew: ${g.added.length} entr${g.added.length === 1 ? 'y is' : 'ies are'} not in the baseline at the merge base ${g.base.slice(0, 9)} (${list}${g.added.length > 8 ? ', ...' : ''}); it may only shrink (ADR 0041 §4)`, 'Remove the entries and fix the sites (wait on the condition, a fake clock, or a waiver with its reason). Growing the baseline needs the user\'s approval.'));
        }
      }
    }
    return out;
  },
};

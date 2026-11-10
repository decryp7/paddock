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
// Not yet (ADR 0041 §4 amendment, "first slice"): Instant/SystemTime/Date.now, recv_timeout/wait_timeout/Deadline::after,
// performance.now, page.clock.install without pauseAt, vi.waitFor under fake timers, the live-status spread, C30's elapsed asserts,
// vm-smoke `observe()`, the ESLint rule.
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { error, warn } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

export const BASELINE_PATH = 'conformance/baselines/C71.json';
const WAIVER = /conformance:\s*hang-guard\s*[—–-]+\s*(\S.{2,})/;
const BOUND = /(\d+(?:\.\d+)?)\s*(ms|milliseconds?|min|mins|minutes?|s|secs?|seconds?)\b/g;
const FAKE_SEAM = /\b(?:FakeClock|ManualPace|TestSeams)\b/;
const TS_CLOCK = /\b(?:useTestClock|frozenClock)\s*\(/;
const TS_TEST = /(?:\.(?:spec|test)\.(?:ts|mts|cts|js|mjs|cjs)$|(?:^|\/)(?:e2e|selftest)\/.+\.(?:ts|mjs|js)$)/;
const SKIP_PATH = /^conformance\/test\/|(?:^|\/)(?:node_modules|dist|target|fixtures)\/|^apps\/[^/]+\/vendor\/|(?:^|\/)crates\/paddock-testkit\//;
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

const defaultGit = (cwd) => (args) => execFileSync('git', args, { cwd, encoding: 'utf8', maxBuffer: 64 << 20, stdio: ['ignore', 'pipe', 'pipe'] });

/**
 * The baseline entries that the baseline at the merge base does not have (a multiset difference by file and hash), or null with the
 * reason it could not be measured. A baseline that does not exist at the base is the first introduction: nothing was added.
 */
export function baselineGrowth(ctx, current) {
  const git = ctx.options.git ?? defaultGit(ctx.root);
  let base;
  let text;
  try {
    const ref = ctx.options.baseRef ?? process.env.PADDOCK_BASE_REF ?? 'origin/main';
    const head = git(['rev-parse', 'HEAD']).trim();
    base = git(['merge-base', 'HEAD', ref]).trim();
    if (base === head) base = git(['rev-parse', 'HEAD^1']).trim(); // on the target branch itself: this commit against its parent
  } catch (e) {
    return { reason: `no merge base (${String(e.message ?? e).split('\n')[0].slice(0, 120)})` };
  }
  try {
    text = git(['show', `${base}:${BASELINE_PATH}`]);
  } catch (e) {
    if (/does not exist|exists on disk, but not in|not in/.test(String(e.stderr ?? e.message))) return { base, added: [] };
    return { reason: `the baseline at ${base.slice(0, 9)} cannot be read` };
  }
  const left = new Map();
  for (const e of JSON.parse(text).sites ?? []) left.set(key(e), (left.get(key(e)) ?? 0) + 1);
  const added = [];
  for (const e of current) {
    if ((left.get(key(e)) ?? 0) > 0) left.set(key(e), left.get(key(e)) - 1);
    else added.push(e);
  }
  return { base, added };
}

export default {
  id: 'C71',
  title: 'Tests do not settle on real time',
  rule: 49,
  scope: 'both',
  mode: 'gate',
  run(ctx) {
    if (!ctx.config.timingLint) return [];
    const sites = timingSites(ctx.files(), ctx.read);
    let baseline = { sites: [] };
    const raw = ctx.read(BASELINE_PATH);
    if (raw) baseline = JSON.parse(raw);
    const { fresh, stale } = againstBaseline(sites, baseline.sites);
    const out = fresh.map((s) =>
      error('C71', 49, s.file, s.line, `${s.what} (ER-12, rule 49): ${s.text.trim().slice(0, 100)}`, 'Wait on the condition (an event, `expect.poll`, a locator assertion, `useTestClock`/`frozenClock`, the FakeClock), or for a real bound add `// conformance: hang-guard — <why>, <bound of at least 10 s>` on the line.'),
    );
    for (const e of stale) {
      out.push(warn('C71', 49, e.file, e.line ?? 1, `${BASELINE_PATH} lists a site that is gone (hash ${e.hash}); the baseline may only shrink`, 'Remove the entry (`node conformance/bin/paddock-timing-baseline.mjs --write` rewrites the file without it).'));
    }
    if (baseline.sites.length) {
      const g = baselineGrowth(ctx, baseline.sites);
      if (g.reason) {
        out.push(warn('C71', 49, BASELINE_PATH, 1, `the baseline's shrink-only rule is not enforced here: ${g.reason}`, 'Fetch the target branch (`git fetch origin main`, full history), or set PADDOCK_BASE_REF to the ref to compare with.'));
      } else if (g.added.length) {
        const list = g.added.slice(0, 8).map((e) => `${e.file}:${e.line ?? '?'}`).join(', ');
        out.push(error('C71', 49, BASELINE_PATH, 1, `the baseline grew: ${g.added.length} entr${g.added.length === 1 ? 'y is' : 'ies are'} not in the baseline at the merge base ${g.base.slice(0, 9)} (${list}${g.added.length > 8 ? ', ...' : ''}); it may only shrink (ADR 0041 §4)`, 'Remove the entries and fix the sites (wait on the condition, a fake clock, or `// conformance: hang-guard — <bound of at least 10 s>`). Growing the baseline needs the user\'s approval.'));
      }
    }
    return out;
  },
};

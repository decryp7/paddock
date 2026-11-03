// C71: tests do not wait on real time as a settle (ER-12, rule 5, rule 49, ADR 0041). Opt-in with `"timingLint": true` in
// paddock.conformance.json (the Paddock repo turns it on and scans every app below it). Flags, in test code only:
//   TypeScript specs, tests and e2e files: `waitForTimeout(`; a `setTimeout` used as a wait (`await new Promise(r => setTimeout(r, n))`,
//     `await sleep(n)` / `delay(n)`); a wait on `requestAnimationFrame` turns; and a locator count read once and used later to index
//     (`const n = await rows.count()` ... `rows.nth(n - 1)` or a `for (i < n)` loop over `.nth(i)`): the count can change between the
//     two calls (lesson "A sidebar spec counted the links once and indexed past the end"); use `.last()`, `.first()` or an assertion
//     on the count instead.
//   Rust tests (`tests/`, `benches/`, `tests.rs`, `*_tests.rs`, and everything after `#[cfg(test)]`): `thread::sleep` and
//     `tokio::time::sleep` (a settle: wait on the event, a channel or the FakeClock).
// A real backoff or an observation window is allowed with `// conformance: real-wait — <why>` on the line or the line above
// (`// conformance: hang-guard — <why>` of C30 is accepted too). `paddock-testkit` owns real time and is skipped.
// Existing sites sit in `conformance/baselines/C71.json` as {file, hash of the normalised line}: a site in the baseline passes (until
// the baseline's date), any other is an error. The baseline may only shrink: a listed site that is gone is a warning to remove it.
import { createHash } from 'node:crypto';
import { error, warn } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

export const BASELINE_PATH = 'conformance/baselines/C71.json';
const WAIVER = /conformance:\s*(?:real-wait|hang-guard)\s*[—–-]+\s*\S.{2,}/;
const TS_TEST = /(?:\.(?:spec|test)\.(?:ts|mts|cts|js|mjs|cjs)$|(?:^|\/)(?:e2e|selftest)\/.+\.(?:ts|mjs|js)$)/;
const SKIP_PATH = /^conformance\/test\/|(?:^|\/)(?:node_modules|dist|target|fixtures)\/|(?:^|\/)crates\/paddock-testkit\//;
const RUST_TEST_FILE = /(?:^|\/)(?:tests|benches)\/.+\.rs$|(?:^|\/)tests\.rs$|_tests?\.rs$/;
const SLEEP_FN = /\b(?:std::)?thread::sleep\s*\(|\b(?:tokio::)?time::sleep\s*\(/;

/** Hash of a line's text with whitespace collapsed, so a site is known by what it says and survives moving. */
export const lineHash = (text) => createHash('sha256').update(text.replace(/\s+/g, ' ').trim()).digest('hex').slice(0, 16);

const waived = (lines, i) => WAIVER.test(lines[i] ?? '') || WAIVER.test(lines[i - 1] ?? '');

/** Blanks `//` and block comments of TypeScript (newlines kept) so commented-out code is not flagged. String contents are kept. */
function maskTsComments(text) {
  return text.replace(/\/\*[\s\S]*?\*\/|\/\/[^\n]*/g, (m, off) => (m.startsWith('//') && text[off - 1] === ':' ? m : m.replace(/[^\n]/g, ' ')));
}

/** The timing sites of one TypeScript test file: [{ line, what }] without waivers. */
export function tsTimingSites(source) {
  const lines = source.split('\n');
  const code = maskTsComments(source).split('\n');
  const out = [];
  const add = (i, what) => {
    if (!waived(lines, i)) out.push({ line: i + 1, what });
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

/** The sleeps in the test code of one Rust file: [{ line, what }] without waivers. */
export function rustTimingSites(source, { wholeFileIsTest = false } = {}) {
  const lines = source.split('\n');
  const masked = maskRust(source).split('\n');
  const testFrom = wholeFileIsTest ? 0 : masked.findIndex((l) => /#\[cfg\(test\)\]/.test(l));
  if (testFrom < 0) return [];
  const bareSleep = /\buse\s+(?:std::thread|tokio::time)::(?:\{[^}]*\bsleep\b[^}]*\}|sleep)\b/.test(masked.join('\n'));
  const out = [];
  for (let i = testFrom; i < masked.length; i++) {
    const hit = SLEEP_FN.test(masked[i]) || (bareSleep && /(?<![:\w.])sleep\s*\(/.test(masked[i]));
    if (hit && !waived(lines, i)) out.push({ line: i + 1, what: 'a sleep in a test is a settle: wait on the event, a channel or the FakeClock' });
  }
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

export default {
  id: 'C71',
  title: 'Tests do not settle on real time',
  rule: 49,
  run(ctx) {
    if (!ctx.config.timingLint) return [];
    const sites = timingSites(ctx.files(), ctx.read);
    let baseline = { expires: '1970-01-01', sites: [] };
    const raw = ctx.read(BASELINE_PATH);
    if (raw) baseline = JSON.parse(raw);
    const { fresh, stale } = againstBaseline(sites, baseline.sites);
    const out = fresh.map((s) =>
      error('C71', 49, s.file, s.line, `${s.what} (ER-12, rule 49): ${s.text.trim().slice(0, 100)}`, 'Wait on the condition (an event, `expect.poll`, a locator assertion, the FakeClock), or for a real backoff add `// conformance: real-wait — <why>` on the line.'),
    );
    if (baseline.sites.length && ctx.now > new Date(`${baseline.expires}T23:59:59Z`)) {
      out.push(error('C71', 49, BASELINE_PATH, 1, `the baseline of ${baseline.sites.length} known sites expired on ${baseline.expires}`, 'Migrate the sites (each file:line is in the baseline) or move the date with the user\'s approval.'));
    }
    for (const e of stale) {
      out.push(warn('C71', 49, e.file, e.line ?? 1, `${BASELINE_PATH} lists a site that is gone (hash ${e.hash}); the baseline may only shrink`, 'Remove the entry (`node conformance/bin/paddock-timing-baseline.mjs --write` rewrites the file without it).'));
    }
    return out;
  },
};

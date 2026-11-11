import assert from 'node:assert/strict';
import { test } from 'node:test';
import { againstBaseline, retriesWithoutFlakyFailure, lineHash, rustTimingSites, shrinkBaseline, timingSites, tsTimingSites } from '../src/checks/C71-test-timing.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const ts = (t) => tsTimingSites(t).map((s) => s.line);
const rs = (t, o) => rustTimingSites(t, o).map((s) => s.line);

test('C71 flags fixed waits, setTimeout settles, rAF waits and a count used to index, in TypeScript', () => {
  assert.deepEqual(ts("await page.waitForTimeout(500);\n"), [1]);
  assert.deepEqual(ts('await new Promise((r) => setTimeout(r, 300));\n'), [1]);
  assert.deepEqual(ts('await sleep(50);\nawait delay(10);\n'), [1, 2]);
  assert.deepEqual(ts('await new Promise((r) => requestAnimationFrame(() => r(null)));\n'), [1]);
  assert.deepEqual(ts('const n = await rows.count();\nawait rows.nth(n - 1).click();\n'), [2]);
  assert.deepEqual(ts('const count = await rows.count();\nfor (let i = 0; i < count; i++) {\n  await rows.nth(i).click();\n}\n'), [3]);
});

test('C71 passes waived sites, comments, other counts and the safe forms', () => {
  assert.deepEqual(ts('// conformance: hang-guard — a 20 s bound on the fake device\nawait page.waitForTimeout(250);\n'), []);
  assert.deepEqual(ts('await sleep(5); // conformance: hang-guard — 30 s bound, the fake device has no event\n'), []);
  assert.deepEqual(ts('// await page.waitForTimeout(500);\n/* await sleep(5) */\n'), []);
  assert.deepEqual(ts('await expect(rows).toHaveCount(3);\nawait rows.last().click();\nawait rows.nth(2).click();\n'), []);
  assert.deepEqual(ts('const n = await rows.count();\nexpect(n).toBe(3);\nawait rows.nth(0).click();\n'), []);
  assert.deepEqual(ts('await sleep(delayMs);\nconst t = setTimeout(fn, 5);\n'), []);
});

test('C71 flags a sleep in Rust test code only', () => {
  const body = 'fn a() {\n    std::thread::sleep(D);\n}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        thread::sleep(D);\n        tokio::time::sleep(D).await;\n    }\n}\n';
  assert.deepEqual(rs(body), [8, 9]);
  assert.deepEqual(rs('fn t() {\n    thread::sleep(D);\n}\n', { wholeFileIsTest: true }), [2]);
  assert.deepEqual(rs('use std::thread::sleep;\nfn t() {\n    sleep(D);\n}\n', { wholeFileIsTest: true }), [3]);
});

test('C71 passes a real-wait, a hang-guard, a comment and a string in Rust', () => {
  assert.deepEqual(rs('fn t() {\n    // conformance: hang-guard — a 20 s bound on the peer\n    thread::sleep(D);\n}\n', { wholeFileIsTest: true }), []);
  assert.deepEqual(rs('fn t() {\n    thread::sleep(D); // conformance: hang-guard — 1 min bound\n}\n', { wholeFileIsTest: true }), []);
  assert.deepEqual(rs('fn t() {\n    // thread::sleep(D);\n    let s = "thread::sleep(D)";\n}\n', { wholeFileIsTest: true }), []);
  assert.deepEqual(rs('fn t() {\n    thread::sleep(D); // conformance: hang-guard\n}\n', { wholeFileIsTest: true }).length, 1, 'a waiver needs its reason');
});

test('C71 picks test files, skips the testkit and fixtures', () => {
  const files = {
    'ui/e2e/a.spec.ts': 'await page.waitForTimeout(1);\n',
    'ui/src/helper.ts': 'await page.waitForTimeout(1);\n',
    'crates/x/tests/it.rs': 'fn t() { thread::sleep(D); }\n',
    'crates/x/src/lib.rs': 'fn f() { thread::sleep(D); }\n',
    'crates/paddock-testkit/src/time.rs': '#[cfg(test)]\nfn f() { thread::sleep(D); }\n',
    'ui/e2e/fixtures/b.spec.ts': 'await page.waitForTimeout(1);\n',
  };
  const sites = timingSites(Object.keys(files), (f) => files[f]);
  assert.deepEqual(sites.map((s) => s.file), ['ui/e2e/a.spec.ts', 'crates/x/tests/it.rs']);
});

test('C71 baseline: a known site passes by its text wherever it moves, a copy or an edit is new, a gone site is stale', () => {
  const a = { file: 'f.ts', line: 3, hash: lineHash('await  sleep(5);') };
  assert.equal(a.hash, lineHash('await sleep(5);'), 'whitespace does not change the hash');
  const moved = { ...a, line: 40 };
  assert.deepEqual(againstBaseline([moved], [a]), { fresh: [], stale: [] });
  assert.equal(againstBaseline([moved, { ...a, line: 50 }], [a]).fresh.length, 1, 'a second copy is new');
  assert.equal(againstBaseline([{ ...a, hash: lineHash('await sleep(6);') }], [a]).fresh.length, 1, 'an edited line is new');
  assert.equal(againstBaseline([], [a]).stale.length, 1);
});

const firstIntroduction = { git: (args) => {
  if (args[0] === 'rev-parse') return 'def456\n';
  if (args[0] === 'merge-base') return 'abc123\n';
  throw Object.assign(new Error('fatal: path does not exist in abc123'), { stderr: 'fatal: path does not exist in abc123' });
} };

test('C71 through the runner: opt-in, new sites are errors, the baseline passes known ones', async () => {
  const bad = 'await page.waitForTimeout(500);\n';
  const files = { 'paddock.conformance.json': JSON.stringify({ app: 'demo', timingLint: true }), 'e2e/a.spec.ts': bad };
  const off = makeApp({ 'e2e/a.spec.ts': bad });
  assert.deepEqual(await findingsOf('C71', off), [], 'off unless the repo opts in');
  const root = makeApp(files);
  const f = await findingsOf('C71', root, { options: firstIntroduction });
  assert.deepEqual(f.map((x) => [x.severity, x.file, x.line]), [['error', 'e2e/a.spec.ts', 1]]);
  assert.match(f[0].fix, /hang-guard/);
  const known = { sites: [{ file: 'e2e/a.spec.ts', line: 1, hash: lineHash(bad) }] };
  const ok = makeApp({ ...files, 'conformance/baselines/C71.json': JSON.stringify(known) });
  assert.deepEqual(await findingsOf('C71', ok, { options: firstIntroduction }), []);
  const dated = makeApp({ ...files, 'conformance/baselines/C71.json': JSON.stringify({ ...known, expires: '2000-01-01' }) });
  assert.deepEqual(await findingsOf('C71', dated, { options: firstIntroduction }), [], 'the baseline has no expiry (ADR 0041 WP-6 is open-ended)');
  const gone = makeApp({ 'paddock.conformance.json': files['paddock.conformance.json'], 'conformance/baselines/C71.json': JSON.stringify(known) });
  assert.deepEqual((await findingsOf('C71', gone, { options: firstIntroduction })).map((x) => x.severity), ['warn']);
});

test('C71 hang-guard waives only with a bound of at least 10 s; real-wait waives nothing', () => {
  assert.deepEqual(ts('// conformance: hang-guard — 10 s bound\nawait sleep(5);\n'), []);
  assert.deepEqual(ts('// conformance: hang-guard — bounded at 90000 ms\nawait sleep(5);\n'), []);
  assert.deepEqual(ts('// conformance: hang-guard — 2 min limit\nawait sleep(5);\n'), []);
  assert.deepEqual(ts('// conformance: hang-guard — a 5 s bound\nawait sleep(5);\n'), [2], 'under 10 s');
  assert.deepEqual(ts('// conformance: hang-guard — stops a hung test\nawait sleep(5);\n'), [2], 'no bound named');
  assert.deepEqual(ts('// conformance: real-wait — backs off by design\nawait sleep(5);\n'), [2], 'real-wait is not in ADR 0041');
  assert.deepEqual(rs('fn t() {\n    // conformance: real-wait — x y z\n    thread::sleep(D);\n}\n', { wholeFileIsTest: true }), [3]);
});

test('C71 exempts a call under a fake clock: useTestClock / frozenClock in the enclosing scope, a seam as the call argument', () => {
  const under = "describe('a', () => {\n  useTestClock();\n  it('b', async () => {\n    await sleep(50);\n  });\n});\n";
  assert.deepEqual(ts(under), []);
  const test1 = "test('b', async ({ page }) => {\n  await frozenClock(page, 0);\n  await page.waitForTimeout(50);\n});\n";
  assert.deepEqual(ts(test1), []);
  const sibling = "describe('a', () => {\n  it('clocked', () => {\n    useTestClock();\n  });\n  it('real', async () => {\n    await sleep(50);\n  });\n});\n";
  assert.deepEqual(ts(sibling), [6], 'a clock in a sibling test exempts nothing');
  assert.deepEqual(ts("// useTestClock();\nit('b', async () => {\n  await sleep(50);\n});\n"), [3], 'a mention in a comment exempts nothing');
  assert.deepEqual(rs('fn t() {\n    tokio::time::sleep(FakeClock::step()).await;\n}\n', { wholeFileIsTest: true }), []);
  assert.deepEqual(rs('fn t() {\n    thread::sleep(pace.ManualPace::tick());\n}\n', { wholeFileIsTest: true }), []);
  assert.deepEqual(rs('use x::FakeClock;\nfn t() {\n    thread::sleep(D);\n}\n', { wholeFileIsTest: true }), [3], 'a file that mentions a fake clock exempts nothing');
});

test('C71 Rust test code is the #[cfg(test)] item only, not the rest of the file', () => {
  const src = 'fn a() {\n    std::thread::sleep(D);\n}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        thread::sleep(D);\n    }\n}\nfn after() {\n    thread::sleep(D);\n}\n#[cfg(test)]\nmod more {\n    fn u() {\n        thread::sleep(D);\n    }\n}\n';
  assert.deepEqual(rs(src), [7, 16]);
  assert.deepEqual(rs('#[cfg(test)]\nuse std::thread::sleep;\nfn real() {\n    sleep(D);\n}\n'), [], 'a cfg(test) use is not a region');
  assert.deepEqual(rs('#[cfg(test)]\nmod tests { fn t() { thread::sleep(D); } }\nfn real() { thread::sleep(D); }\n'), [2]);
});

test('C71 blanks string contents and handles // inside strings', () => {
  assert.deepEqual(ts("const s = 'await page.waitForTimeout(500)';\n"), []);
  assert.deepEqual(ts('const s = `await sleep(5)`;\n'), []);
  assert.deepEqual(ts("const u = 'http://x.invalid/a'; await page.waitForTimeout(5);\n"), [1], 'code after a string with // is still code');
  assert.deepEqual(ts("const u = '// not a comment'; await sleep(5);\n"), [1]);
  assert.deepEqual(ts('const r = /["\']/; await sleep(5);\n'), [1], 'a regex with quotes does not open a string');
  assert.deepEqual(ts('const t = `a ${await sleep(5)} b`;\n'), [1], 'code in a template substitution is code');
});

test('C71 baseline is shrink-only against the merge base: an added entry is an error, a removed one is fine', async () => {
  const bad = 'await page.waitForTimeout(500);\n';
  const other = 'await page.waitForTimeout(700);\n';
  const entry = (text, line = 1) => ({ file: 'e2e/a.spec.ts', line, hash: lineHash(text) });
  const files = (sites) => ({
    'paddock.conformance.json': JSON.stringify({ app: 'demo', timingLint: true }),
    'e2e/a.spec.ts': bad + other,
    'conformance/baselines/C71.json': JSON.stringify({ sites }),
  });
  const gitWith = (base) => (args) => {
    if (args[0] === 'rev-parse') return 'def456\n';
    if (args[0] === 'merge-base') return 'abc123\n';
    if (args[0] === 'show') {
      if (base === null) throw new Error('fatal: path does not exist');
      return JSON.stringify({ sites: base });
    }
    throw new Error(`unexpected git ${args.join(' ')}`);
  };
  const run = (current, base) => findingsOf('C71', makeApp(files(current)), { options: { git: gitWith(base) } });
  const both = [entry(bad), entry(other, 2)];
  assert.deepEqual(await run(both, both), []);
  assert.deepEqual(await run(both, [entry(bad), entry(other, 2), entry('gone();')]), [], 'shrinking is fine');
  const grown = await run(both, [entry(bad)]);
  assert.deepEqual(grown.map((x) => x.severity), ['error']);
  assert.equal(grown[0].file, 'conformance/baselines/C71.json');
  assert.match(grown[0].message, /e2e\/a\.spec\.ts/);
  assert.match(grown[0].fix, /fix|hang-guard/i);
  const dup = await findingsOf('C71', makeApp(files([entry(bad), entry(bad, 5), entry(other, 2)])), { options: { git: gitWith([entry(bad), entry(other, 2)]) } });
  assert.ok(dup.some((x) => x.severity === 'error' && x.file === 'conformance/baselines/C71.json'), 'a second copy of a known hash is an added entry');
  const noGit = await findingsOf('C71', makeApp(files(both)), { options: { git: () => { throw new Error('not a git repository'); } } });
  assert.deepEqual(noGit.map((x) => x.severity), ['warn'], 'no merge base: not measured, said so');
  assert.match(noGit[0].message, /not (enforced|measured)/);
  const fresh = await run(both, null);
  assert.deepEqual(fresh, [], 'no baseline at the merge base: the first introduction');
});

test('C71 shrinkBaseline keeps the matching entries with their current lines and never adds one', () => {
  const site = (file, line, text) => ({ file, line, hash: lineHash(text) });
  const entries = [site('a.ts', 3, 'await sleep(5);'), site('a.ts', 9, 'await sleep(6);'), site('b.ts', 1, 'gone();')];
  const sites = [site('a.ts', 30, 'await sleep(5);'), site('a.ts', 40, 'await sleep(5);'), site('c.ts', 2, 'await sleep(7);')];
  const kept = shrinkBaseline(sites, entries);
  assert.deepEqual(kept, [{ file: 'a.ts', line: 30, hash: lineHash('await sleep(5);') }], 'a copy and a new file are not added; stale ones go');
});

test('C71 leaves vendored third-party code to its own upstream (a vendor/ folder)', async () => {
  const sleepy = '#[cfg(test)]\nmod t {\n    #[test]\n    fn a() {\n        std::thread::sleep(std::time::Duration::from_secs(1));\n    }\n}\n';
  const files = { 'paddock.conformance.json': JSON.stringify({ app: 'demo', timingLint: true }), 'apps/x/vendor/dep/src/system.rs': sleepy };
  assert.deepEqual(await findingsOf('C71', makeApp(files), { options: firstIntroduction }), []);
  const own = await findingsOf('C71', makeApp({ ...files, 'crates/x/src/vendor/a.rs': sleepy }), { options: firstIntroduction });
  assert.deepEqual(own.map((x) => x.file), ['crates/x/src/vendor/a.rs'], 'a first-party folder named vendor is still checked');
});

test('C71 flags a Playwright config whose retries can exceed 0 without failOnFlakyTests (ER-12), and passes the safe forms', async () => {
  const bad = "export default defineConfig({\n  retries: process.env['CI'] ? 1 : 0,\n  reporter: 'list',\n});\n";
  assert.equal(retriesWithoutFlakyFailure(bad), 2, 'positive control: the unsafe form is seen');
  assert.equal(retriesWithoutFlakyFailure(bad.replace('reporter', "failOnFlakyTests: !!process.env['CI'],\n  reporter")), 0);
  assert.equal(retriesWithoutFlakyFailure(bad.replace('reporter', 'failOnFlakyTests: false,\n  reporter')), 2, 'false is not a safeguard');
  assert.equal(retriesWithoutFlakyFailure('export default defineConfig({\n  retries: 0,\n});\n'), 0);
  assert.equal(retriesWithoutFlakyFailure('export default defineConfig({ retries: 0 });\n'), 0);
  assert.equal(retriesWithoutFlakyFailure('export default defineConfig({\n  // retries: 2,\n});\n'), 0, 'a comment is not a setting');
  assert.equal(retriesWithoutFlakyFailure('export default defineConfig({});\n'), 0);
  const files = (cfg) => ({ 'paddock.conformance.json': JSON.stringify({ app: 'demo', timingLint: true }), 'apps/x/app/playwright.config.ts': cfg });
  const f = await findingsOf('C71', makeApp(files(bad)), { options: firstIntroduction });
  assert.deepEqual(f.map((x) => [x.severity, x.file, x.line]), [['error', 'apps/x/app/playwright.config.ts', 2]]);
  assert.deepEqual(await findingsOf('C71', makeApp(files('export default defineConfig({ retries: 1, failOnFlakyTests: true });\n')), { options: firstIntroduction }), []);
});

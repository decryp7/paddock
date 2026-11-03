import assert from 'node:assert/strict';
import { test } from 'node:test';
import { againstBaseline, lineHash, rustTimingSites, timingSites, tsTimingSites } from '../src/checks/C71-test-timing.mjs';
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
  assert.deepEqual(ts('// conformance: real-wait — the server backs off 200 ms by design\nawait page.waitForTimeout(250);\n'), []);
  assert.deepEqual(ts('await sleep(5); // conformance: real-wait — poll interval of the fake device\n'), []);
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
  assert.deepEqual(rs('fn t() {\n    // conformance: real-wait — the peer backs off 50 ms by design\n    thread::sleep(D);\n}\n', { wholeFileIsTest: true }), []);
  assert.deepEqual(rs('fn t() {\n    thread::sleep(D); // conformance: hang-guard — a 20 s bound\n}\n', { wholeFileIsTest: true }), []);
  assert.deepEqual(rs('fn t() {\n    // thread::sleep(D);\n    let s = "thread::sleep(D)";\n}\n', { wholeFileIsTest: true }), []);
  assert.deepEqual(rs('fn t() {\n    thread::sleep(D); // conformance: real-wait\n}\n', { wholeFileIsTest: true }).length, 1, 'a waiver needs its reason');
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

test('C71 through the runner: opt-in, new sites are errors, the baseline passes known ones and expires', async () => {
  const bad = 'await page.waitForTimeout(500);\n';
  const files = { 'paddock.conformance.json': JSON.stringify({ app: 'demo', timingLint: true }), 'e2e/a.spec.ts': bad };
  const off = makeApp({ 'e2e/a.spec.ts': bad });
  assert.deepEqual(await findingsOf('C71', off), [], 'off unless the repo opts in');
  const root = makeApp(files);
  const f = await findingsOf('C71', root);
  assert.deepEqual(f.map((x) => [x.severity, x.file, x.line]), [['error', 'e2e/a.spec.ts', 1]]);
  assert.match(f[0].fix, /real-wait/);
  const known = { expires: '2999-01-01', sites: [{ file: 'e2e/a.spec.ts', line: 1, hash: lineHash(bad) }] };
  const ok = makeApp({ ...files, 'conformance/baselines/C71.json': JSON.stringify(known) });
  assert.deepEqual(await findingsOf('C71', ok), []);
  const old = makeApp({ ...files, 'conformance/baselines/C71.json': JSON.stringify({ ...known, expires: '2000-01-01' }) });
  assert.deepEqual((await findingsOf('C71', old)).map((x) => x.file), ['conformance/baselines/C71.json']);
  const gone = makeApp({ 'paddock.conformance.json': files['paddock.conformance.json'], 'conformance/baselines/C71.json': JSON.stringify(known) });
  assert.deepEqual((await findingsOf('C71', gone)).map((x) => x.severity), ['warn']);
});

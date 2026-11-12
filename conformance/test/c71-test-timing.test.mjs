import assert from 'node:assert/strict';
import { test } from 'node:test';
import { againstBaseline, baselineGrowth, retriesWithoutFlakyFailure, lineHash, rustTimingSites, rustWaitSites, rustRaceSites, tsRaceSites, tsWaitSites, shrinkBaseline, timingSites, tsTimingSites } from '../src/checks/C71-test-timing.mjs';
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
    // The kit's wait for a started program is its own crate (ci.yml 7989); it owns real time too.
    'crates/paddock-testkit-start/src/lib.rs': '#[cfg(test)]\nfn f() { thread::sleep(D); }\n',
    'crates/paddock-testkit-starter/src/tests.rs': '#[cfg(test)]\nfn f() { thread::sleep(D); }\n',
    'ui/e2e/fixtures/b.spec.ts': 'await page.waitForTimeout(1);\n',
  };
  const sites = timingSites(Object.keys(files), (f) => files[f]);
  assert.deepEqual(sites.map((s) => s.file), ['ui/e2e/a.spec.ts', 'crates/x/tests/it.rs', 'crates/paddock-testkit-starter/src/tests.rs'], 'only the two kit crates, not a name that starts like them');
});

test('C71 baseline: a known site passes by its text wherever it moves, a copy or an edit is new, a gone site is stale', () => {
  const a = { file: 'f.ts', line: 3, hash: lineHash('await  sleep(5);') };
  assert.equal(a.hash, lineHash('await sleep(5);'), 'whitespace does not change the hash');
  const moved = { ...a, line: 40 };
  assert.deepEqual(againstBaseline([moved], [a]), { fresh: [], stale: [], moved: [] });
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

test('C71 baseline: code moved to another file is moved, not new and not stale; shrinkBaseline rewrites its file', () => {
  const site = (file, line, text) => ({ file, line, hash: lineHash(text) });
  const old = site('a.rs', 3, 'run_waited(x);');
  const now = site('b.rs', 9, 'run_waited(x);');
  const r = againstBaseline([now], [old]);
  assert.deepEqual([r.fresh, r.stale], [[], []]);
  assert.deepEqual(r.moved, [{ item: now, from: old }]);
  assert.deepEqual(shrinkBaseline([now], [old]), [{ file: 'b.rs', line: 9, hash: now.hash }]);
  // moved plus one genuinely new line: the new one is fresh
  const extra = site('b.rs', 20, 'kill(y);');
  assert.deepEqual(againstBaseline([now, extra], [old]).fresh, [extra]);
});

const growthCtx = (base) => ({ root: '.', options: { git: (args) => (args[0] === 'rev-parse' ? 'def\n' : args[0] === 'merge-base' ? 'abc\n' : base) } });

test('C71 baselineGrowth: a moved entry (same hash, another file) is not growth; moved + one new is 1; duplicates are counted per hash', () => {
  const e = (file, text) => ({ file, line: 1, hash: lineHash(text) });
  const at = (...entries) => JSON.stringify({ sites: entries });
  const base = at(e('a.rs', 'x();'), e('c.rs', 'z();'));
  const moved = baselineGrowth(growthCtx(base), [e('b.rs', 'x();'), e('c.rs', 'z();')]);
  assert.deepEqual(moved.added, []);
  assert.equal(moved.moved.length, 1);
  assert.equal(baselineGrowth(growthCtx(base), [e('b.rs', 'x();'), e('c.rs', 'z();'), e('d.rs', 'n();')]).added.length, 1, 'moved + one new is growth of 1');
  // the same line in two files: one removed, one added elsewhere is a move; a third copy is growth
  const dup = at(e('a.rs', 'x();'), e('b.rs', 'x();'));
  assert.equal(baselineGrowth(growthCtx(dup), [e('a.rs', 'x();'), e('c.rs', 'x();')]).added.length, 0);
  assert.equal(baselineGrowth(growthCtx(dup), [e('a.rs', 'x();'), e('b.rs', 'x();'), e('c.rs', 'x();')]).added.length, 1);
  // a file-only difference with a different text is still growth
  assert.equal(baselineGrowth(growthCtx(base), [e('a.rs', 'x();'), e('c.rs', 'other();')]).added.length, 1);
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

// ADR 0071 section 4, R2 and R3 in their smallest form: a fixed Duration under 40 s given to a wait or to a start/connect/ready setting
// in test code. Its own baseline (conformance/baselines/C71-waits.json), shrink-only like C71's.
const waits = (t, o = { wholeFileIsTest: true }) => rustWaitSites(t, o).map((s) => s.line);

test('C71 waits: a fixed Duration under 40 s given to a wait helper or a start setting is flagged, in test code only', () => {
  const t = (body) => `fn t() {\n${body}\n}\n`;
  assert.deepEqual(waits(t('    wait_until(|| up(), Duration::from_secs(10));')), [2]);
  assert.deepEqual(waits(t('    let m = rx.recv_timeout(Duration::from_millis(500));')), [2]);
  assert.deepEqual(waits(t('    child.wait_timeout(Duration::from_secs(5))?;')), [2]);
  assert.deepEqual(waits(t('    let cfg = Config { connect_window: Duration::from_secs(15), ..d };')), [2]);
  assert.deepEqual(waits(t('    spec.ready_timeout = Duration::from_secs(20);')), [2]);
  assert.deepEqual(waits(t('    wait_until(\n        || up(),\n        Duration::from_secs(30),\n    );')), [2], 'a call over several lines is one site, at its first line');
  assert.deepEqual(waits('fn a() {\n    wait_until(|| up(), Duration::from_secs(10));\n}\n', { wholeFileIsTest: false }), [], 'not test code');
  assert.deepEqual(waits('fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        wait_for(|| ok(), Duration::from_secs(3));\n    }\n}\n', { wholeFileIsTest: false }), [5]);
});

test('C71 waits: the ADR 0071 condition wait, a guard of 40 s or more, a fake seam, a waiver, a comment and an unrelated Duration pass', () => {
  const t = (body) => `fn t() {\n${body}\n}\n`;
  assert.deepEqual(waits(t('    wait_ready_or_exit(&mut child, HANG_GUARD)?;')), [], 'a named guard is not a literal budget');
  assert.deepEqual(waits(t('    wait_until(|| up(), Duration::from_secs(40));')), [], '40 s is a hang guard, not a settle');
  assert.deepEqual(waits(t('    let cfg = Config { connect_window: UNTIL_CONNECTED, ..d };')), []);
  assert.deepEqual(waits(t('    wait_until(|| up(), Duration::from_secs(10), &FakeClock::new());')), []);
  assert.deepEqual(waits(t('    // conformance: expiry-under-test — the test examines a 2 s window\n    wait_for(|| gone(), Duration::from_secs(2));')), []);
  assert.deepEqual(waits(t('    // wait_until(|| up(), Duration::from_secs(10));\n    let s = "recv_timeout(Duration::from_secs(1))";')), []);
  assert.deepEqual(waits(t('    let interval = Duration::from_secs(1);\n    let cfg = Config { retry_delay: Duration::from_millis(5), ..d };')), []);
  assert.deepEqual(waits(t('    // conformance: hang-guard — a 20 s bound\n    wait_for(|| gone(), Duration::from_secs(20));')), [3], 'R2 accepts no hang-guard waiver (ADR 0071 section 4)');
});

test('C71 waits through the runner: a new site is an error naming the condition wait, the baseline passes known ones and grows never', async () => {
  const bad = '    wait_until(|| up(), Duration::from_secs(10));\n';
  const source = `fn t() {\n${bad}}\n`;
  const files = { 'paddock.conformance.json': JSON.stringify({ app: 'demo', timingLint: true }), 'crates/x/tests/it/a.rs': source };
  const f = await findingsOf('C71', makeApp(files), { options: { git: (a) => { if (a[0] === 'show') throw new Error('fatal: path does not exist'); return 'abc\n'; } } });
  assert.deepEqual(f.map((x) => [x.severity, x.file, x.line]), [['error', 'crates/x/tests/it/a.rs', 2]]);
  assert.match(f[0].message, /fixed Duration/);
  assert.match(f[0].fix, /wait_ready_or_exit|wait_until|ADR 0071/);
  const known = { sites: [{ file: 'crates/x/tests/it/a.rs', line: 2, hash: lineHash(bad) }] };
  const withBaseline = makeApp({ ...files, 'conformance/baselines/C71-waits.json': JSON.stringify(known) });
  const git = (base) => (a) => { if (a[0] === 'show') { if (base === null) throw new Error('fatal: path does not exist'); return JSON.stringify(base); } return 'abc\n'; };
  assert.deepEqual(await findingsOf('C71', withBaseline, { options: { git: git(known) } }), []);
  const grown = await findingsOf('C71', withBaseline, { options: { git: git({ sites: [] }) } });
  assert.deepEqual(grown.map((x) => [x.severity, x.file]), [['error', 'conformance/baselines/C71-waits.json']]);
});

test('C71 waits in TypeScript: a fixed timeout under 40 s on a wait is flagged; a test option, a longer guard, a clock and a waiver are not', () => {
  const w = (t) => tsWaitSites(t).map((s) => s.line);
  assert.deepEqual(w("await vi.waitFor(() => expect(x()).toBe(1), { timeout: 5000 });\n"), [1]);
  assert.deepEqual(w("await vi.waitFor(\n  () => ok(),\n  { timeout: 10_000 },\n);\n"), [1]);
  assert.deepEqual(w("await expect(async () => { await a(); }).toPass({ timeout: 8000 });\n"), [1]);
  assert.deepEqual(w("await expect.poll(() => n(), { timeout: 2000 }).toBe(3);\n"), [1]);
  assert.deepEqual(w("it('slow', { timeout: 30000 }, async () => {});\n"), [], "a test's own timeout is the runner's guard");
  assert.deepEqual(w("await vi.waitFor(() => ok(), { timeout: 60_000 });\n"), [], '40 s or more is a hang guard');
  assert.deepEqual(w("await vi.waitFor(() => ok());\n"), []);
  assert.deepEqual(w("// conformance: expiry-under-test — the toast hides after 3 s\nawait vi.waitFor(() => gone(), { timeout: 4000 });\n"), []);
  assert.deepEqual(w("// await vi.waitFor(() => ok(), { timeout: 5000 });\n"), []);
  assert.deepEqual(w("useTestClock();\nawait vi.waitFor(() => ok(), { timeout: 5000 });\n"), []);
});

// The class "a test asserts on state another thread writes, right after waiting on a different condition" (ER-21: Debrief's ticker
// test, CI 7963, fixed in 0e162bcdd; Dyno 2026-11-01, lessons.md). Own baseline conformance/baselines/C71-races.json.
const RACE_OLD = `#[test]
fn probes_and_reports_reach_the_snapshot_through_the_ticker() {
    clock.advance(VISIBLE_AFTER_MS);
    wait_until("the probe", || jobs.info(h.id).and_then(|i| i.progress.done) == Some(3));
    assert_eq!(jobs.info(h.id).unwrap().message.as_deref(), Some("reading app.log"));
    let before = changes.load(Ordering::SeqCst);
}
`;
const RACE_FIXED = `#[test]
fn probes_and_reports_reach_the_snapshot_through_the_ticker() {
    wait_until("the probe", || jobs.info(h.id).and_then(|i| i.progress.done) == Some(3));
    assert_eq!(jobs.info(h.id).unwrap().message, None, "the job has not reported yet");
    report.send(()).unwrap();
    wait_until("the job's message", || {
        jobs.info(h.id).and_then(|i| i.message).as_deref() == Some("reading app.log")
    });
    let before = changes.load(Ordering::SeqCst);
}
`;
const races = (t) => rustRaceSites(t, { wholeFileIsTest: true }).map((s) => s.line);

test('C71 races (Rust): the old ticker test is flagged at the assertion on a field the wait did not read; the fixed test is not', () => {
  assert.deepEqual(races(RACE_OLD), [5]);
  assert.deepEqual(races(RACE_FIXED), []);
});

test('C71 races (Rust): the same field, an absence check, an assertion not right after the wait, and non-test code pass', () => {
  const t = (body) => `fn t() {\n${body}\n}\n`;
  assert.deepEqual(races(t('    wait_until("up", || s.info().state == Up);\n    assert_eq!(s.info().state, Up);')), []);
  assert.deepEqual(races(t('    wait_until("up", || s.info().state == Up);\n    let x = 1;\n    assert_eq!(s.info().other, 3);')), [], 'only the statement right after');
  assert.deepEqual(races(t('    wait_until("up", || s.info().state == Up);\n    assert!(s.info().other.is_none());')), []);
  assert.deepEqual(races(t('    wait_for(|| a.count.load() > 1);\n    assert_eq!(h.id, 7);')), [3], 'a different field after a wait_for');
  assert.deepEqual(rustRaceSites('fn a() {\n    wait_until("u", || s.x == 1);\n    assert_eq!(s.y, 2);\n}\n').length, 0, 'not test code');
});

const DYNO_OLD = `it('ghost', async () => {
  btn(r.el, 'Scan')!.click();
  await settle(r.f);
  await vi.advanceTimersByTimeAsync(0);
  r.store.receive({ ...r.store.status()!, seq: 70, state: 'running' });
});
`;
const DYNO_FIXED = `it('ghost', async () => {
  btn(r.el, 'Scan')!.click();
  await vi.waitFor(async () => {
    await settle(r.f);
    expect(r.store.status()?.state).toBe('done');
  });
  r.store.receive({ ...r.store.status()!, seq: 70, state: 'running', hiddenBytes: null, scannedAt: null });
});
`;
test('C71 races (TypeScript): a spread of the current status right after a timer or settle wait is flagged; after a wait that reads that status it is not', () => {
  const r = (t) => tsRaceSites(t).map((s) => s.line);
  assert.deepEqual(r(DYNO_OLD), [5]);
  assert.deepEqual(r(DYNO_FIXED), []);
  assert.deepEqual(r("store.set({ ...store.status()!, seq: 1 });\n"), [], 'no wait before it');
  assert.deepEqual(r("await settle(f);\nstore.set({ ...fresh, seq: 1 });\n"), [], 'not a spread of a getter');
});

test('C71 races (Rust): the wait\'s own return value, a turbofish, the receiver of the wait and an `ordered` waiver are not races', () => {
  const t = (body) => `fn t() {\n${body}\n}\n`;
  // debrief-cli progress.rs:116 and dyno bench.rs:1734: the assertion reads what the wait returned.
  assert.deepEqual(races(t('    let report = wait_until(done, false, &mut out, || false).unwrap();\n    assert_eq!(report.events, 7);')), []);
  assert.deepEqual(races(t('    let seen = c\n        .wait_for(D, |o| o.running.is_some())\n        .and_then(|o| o.running)\n        .expect("no phase");\n    assert_eq!((seen.phase.as_str(), seen.rep), ("cpu", 2));')), []);
  // debrief-core events/tests.rs:37: `collect::<Vec<_>>()` is a call, not a field.
  assert.deepEqual(races(t('    wait_until("delivery", || seen.lock().unwrap().len() == 100);\n    assert_eq!(*seen.lock().unwrap(), (1..=100).collect::<Vec<_>>());')), []);
  // debrief renderer.rs:948: the receiver of the wait (`r.renderers`) is read by it.
  assert.deepEqual(races(t('    assert_eq!(r.renderers.wait_for("main", SEEN, |p| p != Phase::None), Phase::Launching);\n    assert!(r.renderers.kill("main"), "running");')), []);
  assert.deepEqual(races(t('    // conformance: ordered — the job reports before it exits\n    wait_until("up", || s.info().state == Up);\n    assert_eq!(s.info().other, 3);')), [], 'a waiver above the wait');
  assert.deepEqual(races(t('    wait_until("up", || s.info().state == Up);\n    // conformance: ordered — set before the state\n    assert_eq!(s.info().other, 3);')), [], 'a waiver above the assertion');
  assert.deepEqual(races(t('    wait_until("up", || s.info().state == Up);\n    // conformance: ordered\n    assert_eq!(s.info().other, 3);')).length, 1, 'a waiver needs its reason');
  assert.deepEqual(races(t('    let n = wait_until("up", || s.info().state == Up);\n    assert_eq!(s.info().other, 3);')), [3], 'a bound name the assertion does not use');
});

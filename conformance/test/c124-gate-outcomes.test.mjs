import assert from 'node:assert/strict';
import { test } from 'node:test';
import { flaggedJs, flaggedRust, matchingEntries, parseAllow } from '../src/checks/C124-gate-outcomes.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const DEV = 'tools/paddock-dev/src/x/mod.rs';

test('C124 bad: a local result enum, a JSON result object, a bool on a result, a call outside its owner', () => {
  assert.deepEqual(flaggedRust(DEV, 'pub enum Verdict {\n    Green { run: u64 },\n    NotMeasured(String),\n}\n').map((f) => f.line), [2, 3]);
  assert.deepEqual(flaggedRust(DEV, 'enum S { Pass, Failed }\n'), []); // one-line enums are not read (the ids are on the lines of a block)
  assert.equal(flaggedRust(DEV, 'let v = json!({"status": "clean"});\n').length, 1);
  assert.equal(flaggedRust(DEV, 'let v = json!({"verdict": kind, "id": id});\n').length, 1);
  assert.equal(flaggedRust(DEV, 'impl R {\n    pub fn fails(&self) -> bool { true }\n}\n').length, 1);
  assert.equal(flaggedRust(DEV, 'let e = Evidence::ci_run(a, b);\n').length, 1);
  assert.equal(flaggedRust(DEV, 'let o = Proof::adopt(e, &repo);\n').length, 1);
});

test('C124 good: the owner, the allowed callers, test code, comments, other keys', () => {
  assert.deepEqual(flaggedRust('crates/paddock-cli/src/outcome/mod.rs', 'enum X {\n    NotMeasured,\n}\n'), []);
  assert.deepEqual(flaggedRust('tools/paddock-dev/src/pre_push/known.rs', 'let o = Proof::adopt(e, &repo);\n'), []);
  assert.deepEqual(flaggedRust('tools/paddock-dev/tests/it/a.rs', 'let v = json!({"status": "clean"});\n'), []);
  assert.deepEqual(flaggedRust(DEV, 'fn x() {}\n#[cfg(test)]\nmod tests {\n    fn t() { json!({"status": 1}); }\n}\n'), []);
  assert.deepEqual(flaggedRust(DEV, '// json!({"status": "clean"})\nlet v = json!({"id": 1, "state": "x"});\n'), []);
  assert.deepEqual(flaggedRust(DEV, 'enum K {\n    Other { x: u8 },\n}\nlet clean = 1;\n'), []);
});

test('C124 JS: a pass made by hand is flagged outside outcome.mjs, comments and tests are not', () => {
  assert.equal(flaggedJs('conformance/src/runner.mjs', "return { status: 'clean', findings: [] };\n").length, 1);
  assert.deepEqual(flaggedJs('conformance/src/outcome.mjs', "const outcome = { status: found.length ? 'findings' : 'clean' };\n"), []);
  assert.deepEqual(flaggedJs('conformance/src/runner.mjs', "// status: 'clean'\n"), []);
  assert.deepEqual(flaggedJs('conformance/test/a.test.mjs', "const r = { status: 'clean' };\n"), []);
});

test('C124 bad: a measurement that always returns an empty list', () => {
  assert.equal(flaggedRust(DEV, 'let o = measure(id, s, t, now, || Ok(Vec::new()));\n').length, 1);
  assert.equal(flaggedRust(DEV, 'let o = measure(id, s, t, now, || Ok(vec![]));\n').length, 1);
  const multi = 'let o = measure(\n    id,\n    s,\n    || {\n        Ok(Vec::default())\n    },\n);\n';
  assert.deepEqual(flaggedRust(DEV, multi).map((f) => [f.line, f.text]), [[1, 'let o = measure(']]); // the `measure(` line is the site
  assert.equal(flaggedRust(DEV, 'let o = measure(id, s, t, now, || Ok(Default::default()));\n').length, 1);
  // comments are not code, and the match ends with the call: the next arm is another measurement
  assert.equal(flaggedRust(DEV, '// measure(a, || Ok(Vec::new()))\nlet o = measure(a, b, c, d, || Ok(found));\n').length, 0);
  assert.equal(flaggedRust(DEV, 'let o = measure(a, b, c, d, || Ok(found));\nlet p = other(|| Ok(Vec::new()));\n').length, 0);
  assert.equal(flaggedRust(DEV, 'let o = measure(a, b, c, d, || {\n    // Ok(Vec::new())\n    Ok(found)\n});\n').length, 0);
  assert.equal(flaggedRust(DEV, 'let o = measure(id, s, t, now, || Ok(vec![Finding::failing(c)]));\n').length, 0);
  assert.equal(flaggedRust(DEV, 'let o = measure(id, s, t, now, || Ok(found));\n').length, 0);
  assert.equal(flaggedRust(DEV, 'let a = Ok(Vec::new());\nlet o = measure(id, s, t, now, || Ok(found));\n').length, 0);
});

test('C124 allow list: per site, never a whole file', () => {
  const allow = parseAllow('{"allow": [{"path": "a/b.rs", "site": "|| Ok(Vec::new())", "reason": "a reason that is long enough"}]}');
  assert.deepEqual(allow, [{ path: 'a/b.rs', site: '|| Ok(Vec::new())', reason: 'a reason that is long enough' }]);
  assert.deepEqual(matchingEntries(allow, 'a/b.rs', 'x(|| Ok(Vec::new()))'), [0]);
  assert.deepEqual(matchingEntries(allow, 'a/b.rs', 'enum Verdict { NotMeasured }'), []);
  assert.deepEqual(matchingEntries(allow, 'a/c.rs', 'x(|| Ok(Vec::new()))'), []);
  assert.throws(() => parseAllow('{"allow": [{"path": "a/b.rs", "reason": "a reason that is long enough"}]}'), /site/);
  assert.throws(() => parseAllow('{"allow": [{"path": "a", "site": "x"}]}'), /reason/);
  assert.throws(() => parseAllow('nope'), /not JSON/);
});

test('C124 through the runner: warnings only, the allow list spares a file, a missing list is a broken check', async () => {
  const bad = 'pub enum V {\n    NotMeasured,\n}\n';
  const root = makeApp({
    'conformance/c124-allow.json': '{"allow": [{"path": "tools/paddock-dev/src/ok.rs", "site": "NotMeasured", "reason": "the seed, shrink only"}]}',
    [DEV]: bad,
    // a listed file does not hide a different, unlisted site in it
    'tools/paddock-dev/src/ok.rs': `${bad}fn f() { let o = measure(a, b, c, d, || Ok(Vec::new())); }\n`,
  });
  const found = await findingsOf('C124', root);
  assert.deepEqual(found.map((f) => [f.file, f.severity]).sort(), [['tools/paddock-dev/src/ok.rs', 'warn'], [DEV, 'warn']]);
  assert.match(found.find((f) => f.file === 'tools/paddock-dev/src/ok.rs').message, /empty list/);
  const none = makeApp({ [DEV]: bad });
  const broken = await findingsOf('C124', none);
  assert.equal(broken.some((f) => f.outcome === 'check-broken'), true);
});

test('C124 allow list: a stale entry and an ambiguous entry are warnings', async () => {
  const two = 'enum A {\n    NotMeasured,\n}\nenum B {\n    NotMeasured,\n}\n';
  const f = 'tools/paddock-dev/src/two.rs';
  const allow = (site, path = f) => JSON.stringify({ allow: [{ path, site, reason: 'a reason long enough' }] });
  const stale = await findingsOf('C124', makeApp({ 'conformance/c124-allow.json': allow('Clean,'), [f]: two }));
  assert.equal(stale.some((x) => x.file === 'conformance/c124-allow.json' && /stale/.test(x.message)), true);
  const gone = await findingsOf('C124', makeApp({ 'conformance/c124-allow.json': allow('x', 'tools/paddock-dev/src/gone.rs') }));
  assert.equal(gone.some((x) => /stale/.test(x.message)), true);
  const amb = await findingsOf('C124', makeApp({ 'conformance/c124-allow.json': allow('NotMeasured,'), [f]: two }));
  assert.equal(amb.some((x) => /ambiguous/.test(x.message)), true);
  const ok = await findingsOf('C124', makeApp({ 'conformance/c124-allow.json': allow('NotMeasured,'), [f]: 'enum A {\n    NotMeasured,\n}\n' }));
  assert.deepEqual(ok, []);
});

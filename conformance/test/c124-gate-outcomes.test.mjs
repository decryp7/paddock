import assert from 'node:assert/strict';
import { test } from 'node:test';
import { flaggedJs, flaggedRust, isAllowed, parseAllow } from '../src/checks/C124-gate-outcomes.mjs';
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
  assert.deepEqual(flaggedRust(DEV, multi).map((f) => f.line), [5]);
  assert.equal(flaggedRust(DEV, 'let o = measure(id, s, t, now, || Ok(vec![Finding::failing(c)]));\n').length, 0);
  assert.equal(flaggedRust(DEV, 'let o = measure(id, s, t, now, || Ok(found));\n').length, 0);
  assert.equal(flaggedRust(DEV, 'let a = Ok(Vec::new());\nlet o = measure(id, s, t, now, || Ok(found));\n').length, 0);
});

test('C124 allow list: per site, never a whole file', () => {
  const allow = parseAllow('{"allow": [{"path": "a/b.rs", "site": "|| Ok(Vec::new())", "reason": "a reason that is long enough"}]}');
  assert.deepEqual(allow, [{ path: 'a/b.rs', site: '|| Ok(Vec::new())', reason: 'a reason that is long enough' }]);
  assert.equal(isAllowed(allow, 'a/b.rs', 'x(|| Ok(Vec::new()))'), true);
  assert.equal(isAllowed(allow, 'a/b.rs', 'enum Verdict { NotMeasured }'), false);
  assert.equal(isAllowed(allow, 'a/c.rs', 'x(|| Ok(Vec::new()))'), false);
  assert.throws(() => parseAllow('{"allow": [{"path": "a/b.rs", "reason": "a reason that is long enough"}]}'), /site/);
  assert.throws(() => parseAllow('{"allow": [{"path": "a", "site": "x"}]}'), /reason/);
  assert.throws(() => parseAllow('nope'), /not JSON/);
});

test('C124 through the runner: warnings only, the allow list spares a file, a missing list is a broken check', async () => {
  const bad = 'pub enum V {\n    NotMeasured,\n}\n';
  const root = makeApp({
    'conformance/c124-allow.json': '{"allow": [{"path": "tools/paddock-dev/src/ok.rs", "site": "NotMeasured", "reason": "the seed, shrink only"}]}',
    [DEV]: bad,
    'tools/paddock-dev/src/ok.rs': bad,
    // a listed file does not hide a different site in it
    'tools/paddock-dev/src/part.rs': `${bad}fn f() { let o = measure(a, b, c, d, || Ok(Vec::new())); }\n`,
  });
  const found = await findingsOf('C124', root);
  assert.deepEqual(found.map((f) => [f.file, f.severity]).sort(), [['tools/paddock-dev/src/part.rs', 'warn'], ['tools/paddock-dev/src/part.rs', 'warn'], [DEV, 'warn']]);
  const none = makeApp({ [DEV]: bad });
  const broken = await findingsOf('C124', none);
  assert.equal(broken.some((f) => f.outcome === 'check-broken'), true);
});

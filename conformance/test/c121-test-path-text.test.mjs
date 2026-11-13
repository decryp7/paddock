import assert from 'node:assert/strict';
import { test } from 'node:test';
import C121, { pathTextSites } from '../src/checks/C121-test-path-text.mjs';

const sites = (text, file = 'crates/a/tests/it/x.rs') => pathTextSites(text, file).map((s) => `${s.rule}${s.line}${s.waived ? `:${s.waived}` : ''}`);

test('C121 bad: a format! that joins display() with a literal separator (rule A)', () => {
  assert.deepEqual(sites('fn t() {\n    let e = format!("{}/x", p.display());\n}\n'), ['A2']);
  assert.deepEqual(sites('let e = format!("{}\\\\x", p.display());'), ['A1']);
  assert.deepEqual(sites('let e = format!("a/{}", p.display());'), ['A1']);
  assert.deepEqual(sites('let e = format!("{}/{}", base.display(), name);'), ['A1']);
});

test('C121 good: no display(), no separator next to a placeholder, escapes, comments and strings', () => {
  assert.deepEqual(sites('let e = format!("{}/x", name);'), []);
  assert.deepEqual(sites('let e = format!("{}\\n", p.display());'), []);
  assert.deepEqual(sites('let e = format!("{} {}", a.display(), b);'), []);
  assert.deepEqual(sites('// let e = format!("{}/x", p.display());\n'), []);
  assert.deepEqual(sites('let s = "format!(\\"{}/x\\", p.display())";'), []);
});

test('C121 bad: a hand separator fold (rule B)', () => {
  assert.deepEqual(sites("let t = p.to_string_lossy().replace('\\\\', \"/\");"), ['B1']);
  assert.deepEqual(sites('let t = s.replace("\\\\", "/");'), ['B1']);
  assert.deepEqual(sites('let t = s.replace(r"\\", "/");'), ['B1']);
});

test('C121 good: other replaces', () => {
  assert.deepEqual(sites('let t = s.replace("a", "/");'), []);
  assert.deepEqual(sites('// s.replace(\'\\\\\', "/")\n'), []);
});

test('C121 warn: assert_eq! over path text (rule C), not over a file name', () => {
  assert.deepEqual(sites('assert_eq!(p.display().to_string(), "x");'), ['C1']);
  assert_eq_ok();
  function assert_eq_ok() {
    assert.deepEqual(sites('assert_eq!(p.to_str().unwrap(), "x");'), ['C1']);
    assert.deepEqual(sites('assert_ne!(p.to_string_lossy(), "x");'), ['C1']);
    assert.deepEqual(sites('assert_eq!(p.file_name().unwrap().to_string_lossy(), "x");'), []);
  }
});

test('C121: only test code is read', () => {
  const body = 'fn f() { let e = format!("{}/x", p.display()); }\n';
  assert.deepEqual(sites(body, 'crates/a/src/lib.rs'), []);
  assert.deepEqual(sites(`${body}#[cfg(test)]\nmod tests {\n${body}}\n`, 'crates/a/src/lib.rs'), ['A4']);
  assert.deepEqual(sites(body, 'crates/a/src/tests.rs'), ['A1']);
  assert.deepEqual(sites(body, 'crates/a/src/x_tests.rs'), ['A1']);
  assert.deepEqual(sites(body, 'tools/t/tests/fixtures/x.rs'), []);
  assert.deepEqual(sites(body, 'apps/d/vendor/s/tests/x.rs'), []);
});

test('C121: a waiver with a reason covers the site, one without is flagged', () => {
  const l = 'let e = format!("{}/x", p.display());';
  assert.deepEqual(sites(`// conformance: path-text — a URL path, not a file\n${l}`), ['A2:reason']);
  assert.deepEqual(sites(`${l} // conformance: path-text — a URL path`), ['A1:reason']);
  assert.deepEqual(sites(`// conformance: path-text\n${l}`), ['A2:no-reason']);
});

test('C121 run: rules A and B warn while report-only, nothing is an error', () => {
  const text = 'fn t() {\n let a = format!("{}/x", p.display());\n let b = s.replace(\'\\\\\', "/");\n assert_eq!(p.display().to_string(), "x");\n}\n';
  const ctx = { files: () => ['crates/a/tests/it/x.rs', 'crates/a/src/lib.rs'], read: (f) => (f.endsWith('lib.rs') ? text : text) };
  const found = C121.run(ctx);
  assert.deepEqual(found.map((f) => f.line), [2, 3, 4]);
  assert.ok(found.every((f) => f.severity === 'warn' && f.check === 'C121'));
  assert.equal(C121.mode, 'report-only');
});

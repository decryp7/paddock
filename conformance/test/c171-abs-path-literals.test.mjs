import assert from 'node:assert/strict';
import { test } from 'node:test';
import check, { absPathSites } from '../src/checks/C171-abs-path-literals.mjs';

const lines = (text, file = 'tools/x/tests/it/a.rs') => absPathSites(text, file).map((s) => s.line);

test('C171: a Unix absolute literal passed to Path::new, PathBuf::from or a known validator is flagged', () => {
  assert.deepEqual(lines('fn t() {\n    let p = PathBuf::from("/srv/dev-targets/wt");\n}\n'), [2]);
  assert.deepEqual(lines('fn t() {\n    assert!(Path::new("/a/b").is_absolute());\n}\n'), [2]);
  assert.deepEqual(lines('fn t() {\n    assert_eq!(dev_targets_of(Some("/srv/dt/"), Some("/x/wt")), None);\n}\n'), [2]);
  assert.deepEqual(lines('fn t() {\n    let p = std::path::PathBuf::from(\n        "/dev-targets-x",\n    );\n}\n'), [3]);
});

test('C171: relative paths, text that is not a path constructor, and non-test code are fine', () => {
  assert.deepEqual(lines('fn t() {\n    let p = PathBuf::from("wt/a");\n    let s = String::from("/srv");\n    let u = "/srv/x";\n    let r = Path::new("rel");\n}\n'), []);
  assert.deepEqual(lines('fn t() {\n    // PathBuf::from("/srv/x")\n    let s = "PathBuf::from(\\"/srv\\")";\n}\n'), [], 'comments and strings');
  assert.deepEqual(lines('fn p() { let a = PathBuf::from("/usr/bin"); }\n', 'crates/a/src/lib.rs'), [], 'production code');
});

test('C171: a literal given to a helper that makes it a path of this OS is not flagged', () => {
  assert.deepEqual(lines('fn t() {\n    let p = PathBuf::from(abs("/srv/dt"));\n    let g = dev_targets_of(Some(abs("/srv")), None);\n}\n'), []);
  assert.deepEqual(lines('fn t() {\n    let g = dev_targets_of(None, Some(&"/srv/dt/wt"));\n}\n'), [2], 'a reference and Some() are still direct');
});

test('C171: test items of a src file are read; a unix-only item, a unix-only file and a waiver are not', () => {
  assert.deepEqual(lines('fn p() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { let p = PathBuf::from("/a/b"); }\n}\n', 'crates/a/src/lib.rs'), [5]);
  assert.deepEqual(lines('#[cfg(unix)]\n#[test]\nfn t() { let p = Path::new("/dev/null"); }\n'), [], 'cfg(unix) item');
  assert.deepEqual(lines('#[cfg(not(windows))]\n#[test]\nfn t() { let p = Path::new("/dev/null"); }\n'), [], 'cfg(not(windows)) item');
  assert.deepEqual(lines('#![cfg(unix)]\nfn t() { let p = Path::new("/dev/null"); }\n'), [], 'a unix-only file');
  assert.deepEqual(lines('fn t() {\n    // conformance: abs-path — the validator is given text from a Unix tool\n    let p = Path::new("/a");\n}\n'), [], 'waiver above');
  assert.deepEqual(lines('#[cfg(windows)]\n#[test]\nfn t() { let p = Path::new("/a"); }\n'), [1 + 2], 'a windows-only item is still flagged');
});

test('C171: positive control, a planted case is a warning that points at paddock-testkit::path, and the check is report-only', () => {
  const planted = 'fn t() {\n    let p = PathBuf::from("/planted/abs");\n}\n';
  const files = { 'tools/x/tests/it/planted.rs': planted };
  const ctx = { files: () => Object.keys(files), read: (f) => files[f] };
  const out = check.run(ctx);
  assert.equal(out.length, 1, 'the planted literal is seen');
  assert.equal(out[0].severity, 'warn');
  assert.match(JSON.stringify(out[0]), /paddock-testkit::path/);
  assert.equal(check.mode, 'report-only');
  assert.equal(check.id, 'C171');
  // Hiding the signal (a relative literal) makes it go quiet: the check is not simply always red.
  const quiet = { files: () => ['tools/x/tests/it/p.rs'], read: () => 'fn t() { let p = PathBuf::from("rel/x"); }\n' };
  assert.equal(check.run(quiet).length, 0);
});

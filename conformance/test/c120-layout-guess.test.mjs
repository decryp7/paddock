import assert from 'node:assert/strict';
import { test } from 'node:test';
import { layoutGuesses, unusedWaivers } from '../src/checks/C120-layout-guess.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const lines = (text, shell = false) => layoutGuesses(text, shell).map((g) => g.line);

test('C120 bad: parent hops, dot-dot hops and a literal deps folder', () => {
  assert.deepEqual(lines('let d = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap();\n'), [1]);
  assert.deepEqual(lines('let o = PathBuf::from(env!("OUT_DIR"))\n    .parent()\n    .unwrap()\n    .parent();\n'), [1]);
  assert.deepEqual(lines('let d = current_exe().unwrap().join("../..");\n'), [1]);
  assert.deepEqual(lines('let p = "target/debug/deps/x";\n'), [1]);
  assert.deepEqual(lines('let p = r"C:\\b\\deps\\x";\n'), [1]);
  assert.deepEqual(lines('let q = base.join("deps");\n'), [1]);
  assert.deepEqual(lines('b="$OUT_DIR/../../x"\n', true), [1]);
  assert.deepEqual(lines('x=target/debug/deps/y\n', true), [1]);
});

test('C120 good: one hop, separate statements, comments, the shared helper', () => {
  assert.deepEqual(lines('let h = current_exe().unwrap().parent().unwrap();\n'), []);
  assert.deepEqual(lines('let e = current_exe().unwrap();\nlet d = e.parent().unwrap().parent();\n'), []);
  assert.deepEqual(lines('// current_exe().parent().parent() and /deps/\n'), []);
  assert.deepEqual(lines('# $OUT_DIR/../.. and /deps/\n', true), []);
  assert.deepEqual(lines('let d = cargo_layout::profile_dir_of_exe(&current_exe()?);\n'), []);
});

test('C120 waivers: a reason waives, a bare marker is reported as no reason', () => {
  assert.equal(layoutGuesses('// conformance: layout-guess — pinned layout\nlet q = base.join("deps");\n')[0].waived.state, 'reason');
  assert.equal(layoutGuesses('// conformance: layout-guess\nlet q = base.join("deps");\n')[0].waived.state, 'no-reason');
});

test('C120 waivers carry their reason; a file marker covers test code only, a site marker beats it', () => {
  const st = (x) => [x.line, x.waived.state, x.waived.reason];
  const g = layoutGuesses('// conformance: file-layout-guess — filler data\nlet a = "x/deps/y";\n// conformance: layout-guess — own reason\nlet b = base.join("deps");\n');
  assert.deepEqual(g.map(st), [[2, 'none', undefined], [4, 'reason', 'own reason']], 'line 2 is production code: the file marker does not cover it');
  assert.equal(layoutGuesses('// conformance: file-layout-guess\nlet a = "x/deps/y";\n')[0].waived.state, 'none');
  assert.deepEqual(layoutGuesses('// conformance: file-layout-guess — filler data\nlet a = "x/deps/y";\n', false, 'crates/a/tests/it.rs').map(st), [[2, 'reason', 'filler data']]);
  const t = layoutGuesses('// conformance: file-layout-guess — filler data\nfn p() { let a = "x/deps/y"; }\n#[cfg(test)]\nmod tests {\n    fn t() { let b = "x/deps/y"; }\n}\n');
  assert.deepEqual(t.map((x) => [x.line, x.waived.state]), [[2, 'none'], [5, 'reason']]);
  const other = layoutGuesses('// conformance: file-layout-guess — filler data\n#[cfg(all(test, unix))]\nmod tests {\n    fn t() { let b = "x/deps/y"; }\n}\n');
  assert.deepEqual(other.map((x) => x.waived.state), ['none'], 'cfg(all(test, ..)) is not recognised: the site stays reported');
});

test('C120 unused waivers: a site waiver no guess uses, and a file waiver that waives nothing, are reported at the comment', () => {
  // Used: on the guess's line or the line above.
  assert.deepEqual(unusedWaivers('// conformance: layout-guess — pinned layout\nlet q = base.join("deps");\n'), []);
  assert.deepEqual(unusedWaivers('let q = base.join("deps"); // conformance: layout-guess — pinned layout\n'), []);
  // The guess is gone (or moved two lines down): the waiver would waive whatever is written under it next.
  assert.deepEqual(unusedWaivers('// conformance: layout-guess — pinned layout\nlet q = base.join("cache");\n'), [1]);
  assert.deepEqual(unusedWaivers('// conformance: layout-guess — pinned layout\n\nlet q = base.join("deps");\n'), [1]);
  // A bare marker is already its own finding ("gives no reason"), not an unused one.
  assert.deepEqual(unusedWaivers('// conformance: layout-guess\nlet q = base.join("cache");\n'), []);
  // A file waiver covering a test-module guess is used; one with no covered guess, or a second one, is not.
  const file = '// conformance: file-layout-guess — filler data\n';
  assert.deepEqual(unusedWaivers(`${file}#[cfg(test)]\nmod tests {\n    fn t() { let b = "x/deps/y"; }\n}\n`), []);
  assert.deepEqual(unusedWaivers(`${file}fn p() { let a = 1; }\n`), [1]);
  assert.deepEqual(unusedWaivers(`${file}${file}#[cfg(test)]\nmod tests {\n    fn t() { let b = "x/deps/y"; }\n}\n`), [2]);
  // A site waiver beats the file waiver on its site, so the file waiver there waives nothing.
  assert.deepEqual(unusedWaivers(`${file}#[cfg(test)]\nmod tests {\n    // conformance: layout-guess — own reason\n    fn t() { let b = "x/deps/y"; }\n}\n`), [1]);
  // Shell, and a marker inside a string is data, not a waiver.
  assert.deepEqual(unusedWaivers('# conformance: layout-guess — searches both\necho hi\n', true), [1]);
  assert.deepEqual(unusedWaivers('let s = "// conformance: layout-guess — not a comment";\n'), []);
});

test('C120 through the runner: an unused waiver is a warning in a file with no other trigger', async () => {
  const found = await findingsOf('C120', makeApp({ 'crates/c/src/a.rs': '// conformance: layout-guess — pinned layout\nfn f() {}\n' }));
  assert.deepEqual(found.map((f) => [f.file, f.line, f.severity, /waives no site/.test(f.message)]), [['crates/c/src/a.rs', 1, 'warn', true]]);
});

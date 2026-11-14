import assert from 'node:assert/strict';
import { test } from 'node:test';
import { layoutGuesses } from '../src/checks/C120-layout-guess.mjs';

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

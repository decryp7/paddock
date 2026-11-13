import assert from 'node:assert/strict';
import { test } from 'node:test';
import { layoutGuesses } from '../src/checks/C119-layout-guess.mjs';

const lines = (text, shell = false) => layoutGuesses(text, shell).map((g) => g.line);

test('C119 bad: parent hops, dot-dot hops and a literal deps folder', () => {
  assert.deepEqual(lines('let d = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap();\n'), [1]);
  assert.deepEqual(lines('let o = PathBuf::from(env!("OUT_DIR"))\n    .parent()\n    .unwrap()\n    .parent();\n'), [1]);
  assert.deepEqual(lines('let d = current_exe().unwrap().join("../..");\n'), [1]);
  assert.deepEqual(lines('let p = "target/debug/deps/x";\n'), [1]);
  assert.deepEqual(lines('let p = r"C:\\b\\deps\\x";\n'), [1]);
  assert.deepEqual(lines('let q = base.join("deps");\n'), [1]);
  assert.deepEqual(lines('b="$OUT_DIR/../../x"\n', true), [1]);
  assert.deepEqual(lines('x=target/debug/deps/y\n', true), [1]);
});

test('C119 good: one hop, separate statements, comments, the shared helper', () => {
  assert.deepEqual(lines('let h = current_exe().unwrap().parent().unwrap();\n'), []);
  assert.deepEqual(lines('let e = current_exe().unwrap();\nlet d = e.parent().unwrap().parent();\n'), []);
  assert.deepEqual(lines('// current_exe().parent().parent() and /deps/\n'), []);
  assert.deepEqual(lines('# $OUT_DIR/../.. and /deps/\n', true), []);
  assert.deepEqual(lines('let d = cargo_layout::profile_dir_of_exe(&current_exe()?);\n'), []);
});

test('C119 waivers: a reason waives, a bare marker is reported as no reason', () => {
  assert.equal(layoutGuesses('// conformance: layout-guess — pinned layout\nlet q = base.join("deps");\n')[0].waived, 'reason');
  assert.equal(layoutGuesses('// conformance: layout-guess\nlet q = base.join("deps");\n')[0].waived, 'no-reason');
});

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { MAX_BYTES, parseToml } from '../src/checks/lib/mini-toml.mjs';

const lines = (r) => r.problems.map((p) => p.line);

test('tables and rows have no prototype: __proto__, constructor and hasOwnProperty are data, never inherited or overwritten', () => {
  const r = parseToml('[[__proto__]]\nid = "a"\n[constructor]\nx = 1\n[[feature]]\n__proto__ = "p"\nconstructor = "c"\nhasOwnProperty = 2\ntests = { __proto__ = 1, constructor = 2 }\n');
  assert.deepEqual(r.problems, []);
  assert.equal(Object.getPrototypeOf(r.tables), null);
  assert.equal(Object.getPrototypeOf(r.tables.feature[0]), null);
  assert.equal(Object.getPrototypeOf(r.tables.feature[0].tests), null);
  assert.equal(r.tables.feature[0].__proto__, 'p');
  assert.equal(r.tables.feature[0].constructor, 'c');
  assert.equal(r.tables.feature[0].tests.__proto__, 1);
  assert.equal(r.tables.__proto__[0].id, 'a');
  assert.equal({}.polluted, undefined);
  // an absent key is absent: nothing falls through to Object.prototype
  assert.equal(r.tables.toString, undefined);
  assert.equal(r.tables.feature[0].valueOf, undefined);
});

test('a key repeated across __proto__ spellings is still reported as repeated', () => {
  const r = parseToml('[a]\n__proto__ = 1\n__proto__ = 2\n');
  assert.match(r.problems[0].message, /__proto__ is set twice/);
  assert.match(parseToml('x = { constructor = 1, constructor = 2 }\n').problems[0].message, /set twice in the inline table/);
});

test('a repeated [table] is reported, and so is an array table that was a table', () => {
  const r = parseToml('[caps]\nunlisted = 5\n[caps]\nunlisted = 9\n[[caps]]\n');
  assert.deepEqual(lines(r), [3, 5]);
  assert.match(r.problems[0].message, /the table \[caps\] appears twice/);
  assert.match(r.problems[1].message, /was already a \[caps\] table/);
  assert.equal(r.tables.caps.unlisted, 5, 'the first one is kept');
});

test('an unclosed [ stops at the next column-0 header or key, which is still read', () => {
  const r = parseToml('[[feature]]\nid = "a"\nsurfaces = [\n  "x",\n  "y"\n[[feature]]\nid = "b"\nother = [\n  "z",\nkey = 1\n[caps]\nunlisted = 3\n');
  assert.deepEqual(lines(r), [3, 8]);
  assert.match(r.problems[0].message, /surfaces: a value is not closed before the next header or key/);
  assert.equal(r.tables.feature.length, 2);
  assert.equal(r.tables.feature[1].id, 'b');
  assert.equal(r.tables.feature[1].key, 1);
  assert.equal(r.tables.caps.unlisted, 3);
});

test('an unclosed " is an error on its own line and never swallows the next lines', () => {
  const r = parseToml('a = "open\nb = 2\nc = ["x", "y\nd = 4\n');
  assert.deepEqual(lines(r), [1, 3]);
  assert.match(r.problems[0].message, /a string is not closed/);
  assert.equal(r.tables[''].b, 2);
  assert.equal(r.tables[''].d, 4);
});

test('an array that is closed over several lines, with comments and a trailing comma, still reads; an indented key = line is a continuation', () => {
  const r = parseToml('a = [\n  "x", # c\n  "k = not a key",\n  [1, 2],\n]\nb = 1\n');
  assert.deepEqual(r.problems, []);
  assert.deepEqual([...r.tables[''].a[2]], [1, 2]);
  assert.equal(r.tables[''].a[1], 'k = not a key');
  assert.equal(r.tables[''].b, 1);
});

test('input over 1 MiB is refused with one problem, not read', () => {
  const r = parseToml(`a = "${'x'.repeat(MAX_BYTES)}"\n`);
  assert.equal(r.problems.length, 1);
  assert.match(r.problems[0].message, /larger than 1048576 bytes/);
  assert.deepEqual(Object.keys(r.tables), ['']);
  assert.equal(parseToml(`a = "${'x'.repeat(MAX_BYTES - 20)}"\n`).problems.length, 0);
});

test('nesting deeper than the cap is an error, not a stack overflow', () => {
  const r = parseToml(`a = ${'['.repeat(40000)}${']'.repeat(40000)}\nb = 1\n`);
  assert.equal(r.problems.length, 1);
  assert.match(r.problems[0].message, /nested more than 16 levels/);
  assert.equal(r.tables[''].b, 1);
});

// The time budget is counted in steps the reader takes (its `ops.n`), not in milliseconds: a clock would make this a flaky test.
const budget = (text, perChar = 6) => perChar * text.length + 1000;

test('40k lines of rows and keys are read in a number of steps linear in the input', () => {
  const text = Array.from({ length: 10000 }, (_, i) => `[[feature]]\nid = "f${i}"\ns = ["a", "b"]\nt = { v = ["w"] }\n`).join('');
  assert.equal(text.split('\n').length, 40001);
  const ops = { n: 0 };
  const r = parseToml(text, { ops });
  assert.deepEqual(r.problems, []);
  assert.equal(r.tables.feature.length, 10000);
  assert.ok(ops.n <= budget(text), `${ops.n} steps for ${text.length} bytes`);
});

test('one array over 40k lines, and 40k unclosed [ lines, are linear too', () => {
  const big = `a = [\n${Array.from({ length: 40000 }, (_, i) => `  "item ${i}",`).join('\n')}\n]\n`;
  const o1 = { n: 0 };
  const r1 = parseToml(big, { ops: o1 });
  assert.deepEqual(r1.problems, []);
  assert.equal(r1.tables[''].a.length, 40000);
  assert.ok(o1.n <= budget(big), `${o1.n} steps for ${big.length} bytes`);
  // every line opens an array and the next line is a key = line: each is cut at once, not rescanned from the start
  const bad = Array.from({ length: 20000 }, (_, i) => `k${i} = [\n`).join('');
  const o2 = { n: 0 };
  const r2 = parseToml(bad, { ops: o2 });
  assert.equal(r2.problems.length, 20000);
  assert.ok(o2.n <= budget(bad), `${o2.n} steps for ${bad.length} bytes`);
  // an unclosed array that nothing ends (no header, no key) reads each line once
  const open = `a = [\n${'  "x",\n'.repeat(40000)}`;
  const o3 = { n: 0 };
  assert.equal(parseToml(open, { ops: o3 }).problems.length, 1);
  assert.ok(o3.n <= budget(open), `${o3.n} steps for ${open.length} bytes`);
});

test('random junk never throws and every unreadable line is reported with its line number', () => {
  let seed = 12345;
  const rnd = (n) => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff) % n);
  const atoms = ['[', ']', '[[', ']]', '{', '}', '"', '\\', '=', ',', '#', ' ', '\n', 'a', 'id', '1', 'true', '__proto__', 'constructor', '"x"', ' = ', '\t'];
  for (let k = 0; k < 400; k++) {
    const text = Array.from({ length: 1 + rnd(60) }, () => atoms[rnd(atoms.length)]).join('');
    const ops = { n: 0 };
    const r = parseToml(text, { ops });
    for (const p of r.problems) assert.ok(Number.isInteger(p.line) && p.line >= 1 && typeof p.message === 'string', text);
    assert.ok(ops.n <= budget(text, 8), text);
    assert.equal({}.polluted, undefined);
  }
});

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { regexWithoutU } from '../src/checks/C132-js-regex-flags.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const lines = (text) => regexWithoutU(text).map((s) => s.line);

test('C132 bad: a regular expression without u, in every place one is written', () => {
  assert.deepEqual(lines('const a = /x[^y]{0,5}/;\n'), [1]);
  assert.deepEqual(lines('const a = /x/i;\nconst b = /y/gi;\n'), [1, 2]);
  assert.deepEqual(lines('if (x.test(/a\\/b/)) {}\nreturn /z/;\n'), [1, 2]);
  assert.deepEqual(lines('const a = [/a/, /b/u, /c/];\n'), [1, 1]);
  assert.deepEqual(lines('new RegExp(`a${b}`);\nnew RegExp(x, "g");\nnew RegExp(a(1, 2), \'i\')\n'), [1, 2, 3]);
  assert.deepEqual(lines('const a = /[/]x/;\n'), [1], 'a slash inside a class does not end it');
});

test('C132 good: u flag, strings, comments, division, templates and RegExp with u', () => {
  assert.deepEqual(lines('const a = /x/u; const b = /y/iu; const c = /z/gu;\n'), []);
  assert.deepEqual(lines('const s = "/x/"; const t = \'/y/\'; const u = `/z/ ${1}`;\n'), []);
  assert.deepEqual(lines('// /x/\n/* /y/ */\nconst d = a / b / c;\nconst e = (a) / 2;\n'), []);
  assert.deepEqual(lines('new RegExp(x, "iu");\nnew RegExp(`a`, `gu`);\n'), []);
  assert.deepEqual(lines('new RegExp(x, flags);\n'), [], 'flags that are not a literal are a known gap');
});

test('C132 through the runner: only the checks with a Rust twin, waivers, warnings only', async () => {
  const root = makeApp({
    'conformance/src/checks/C900-a.mjs': 'const a = /x/;\n// conformance: regex-u — ASCII only, both hosts read it alike\nconst b = /y/;\nconst c = /z/u;\n',
    'tools/paddock-dev/src/check/c900.rs': '//! twin\n',
    'conformance/src/checks/C901-b.mjs': 'const a = /x/;\n',
  });
  const found = await findingsOf('C132', root);
  assert.deepEqual(found.map((f) => [f.file, f.line, f.severity]), [['conformance/src/checks/C900-a.mjs', 1, 'warn']], 'C901 has no twin; the waived site and the u site are fine');
  assert.deepEqual(await findingsOf('C132', makeApp({ 'conformance/src/checks/C900-a.mjs': 'const a = /x/u;\n', 'tools/paddock-dev/src/check/c900.rs': '//\n' })), []);
});

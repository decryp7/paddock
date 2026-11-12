import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { tableFindings, splitRow, isDelimiterRow } from '../src/util/md-tables.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const git = (root, ...args) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.invalid', ...args], { cwd: root, encoding: 'utf8' });
const T = (...rows) => rows.join('\n') + '\n';
const OLD_CRATES_README = new URL('./fixtures/crates-readme-broken-table.md.txt', import.meta.url);

test('C110 the old crates README: a blank line inside the table leaves every later row outside it', () => {
  const f = tableFindings(readFileSync(OLD_CRATES_README, 'utf8'));
  assert.ok(f.length >= 25, `rows after the blank line are findings, got ${f.length}`);
  assert.ok(f.every((x) => /outside a table/.test(x.problem)));
  assert.match(f[0].fix, /blank line/);
});

test('C110 a well-formed table, with escaped pipes, alignment and no outer pipes, is clean', () => {
  assert.deepEqual(tableFindings(T('| a | b |', '|:--|--:|', '| `x\\|y` | 2 |', '| 3 | 4 |')), []);
  assert.deepEqual(tableFindings(T('a | b', '--|--', '1 | 2')), []);
  assert.deepEqual(tableFindings(T('| a | b |', '|---|---|', '| 1 | 2 |', '', 'text')), []);
  assert.deepEqual(tableFindings(T('| a | b |', '|---|---|', '| 1 | 2 |', '- list item')), [], 'a list item ends the table');
});

test('C110 a pipe inside a code span splits the cell (GFM), so it must be escaped', () => {
  const f = tableFindings(T('| Format | Notes |', '|---|---|', '| CSV `a|b` | delimited |'));
  assert.equal(f.length, 1);
  assert.equal(f[0].line, 3);
  assert.match(f[0].problem, /3 cells, the header has 2/);
});

test('C110 rows with fewer cells, a header/delimiter mismatch, plain text after the table and a missing closing pipe are findings', () => {
  assert.match(tableFindings(T('| a | b | c |', '|---|---|---|', '| 1 | 2 |'))[0].problem, /2 cells, the header has 3/);
  assert.match(tableFindings(T('| a | b |', '|---|---|---|', '| 1 | 2 |'))[0].problem, /delimiter row has 3/);
  assert.match(tableFindings(T('| a | b |', '|---|---|', '| 1 | 2 |', 'text glued to the table'))[0].problem, /plain text line/);
  assert.match(tableFindings(T('| a | b |', '|---|---|', '| 1 | 2'))[0].problem, /does not end with \|/);
  assert.match(tableFindings(T('|---|---|', '| 1 | 2 |'))[0].problem, /delimiter row without a header/);
});

test('C110 skips fenced code and block quotes are read through', () => {
  assert.deepEqual(tableFindings(T('```', '| a | b |', '', '| 1 | 2 |', '```')), []);
  assert.equal(tableFindings(T('> | a | b |', '> |---|---|', '> | 1 | 2 | 3 |')).length, 1);
});

test('C110 splitRow and isDelimiterRow', () => {
  assert.deepEqual(splitRow('| a \\| b | c |'), [' a \\| b ', ' c ']);
  assert.equal(isDelimiterRow('| --- | :-: |'), true);
  assert.equal(isDelimiterRow('| a | b |'), false);
  assert.equal(isDelimiterRow('---'), false);
});

test('C110 through the runner: tracked .md files only, one error per finding with file:line and the fix', async () => {
  const root = makeApp({ 'ok.md': T('| a |', '|---|', '| 1 |'), 'docs/bad.md': readFileSync(OLD_CRATES_README, 'utf8'), 'x.txt': '| a |\n\n| b |\n' });
  git(root, 'init', '-q');
  git(root, 'add', '.');
  const f = await findingsOf('C110', root);
  assert.ok(f.length >= 25);
  assert.ok(f.every((x) => x.file === 'docs/bad.md' && x.severity === 'error' && x.fix));
  assert.match(f[0].message, /^docs\/bad\.md:\d+: table row outside a table/);
  assert.deepEqual(await findingsOf('C110', makeApp({ 'a.md': '| a |\n\n| b |\n' })), [], 'not a git checkout: nothing to check');
});

test('C110 the real tree is clean', async () => {
  const root = new URL('../..', import.meta.url).pathname;
  assert.deepEqual((await findingsOf('C110', root)).map((x) => `${x.file}:${x.line}`), []);
});

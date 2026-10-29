import assert from 'node:assert/strict';
import { test } from 'node:test';
import { contractsOf, dialogCommandProblems, main } from './dialog-commands.mjs';

const MODULE = `export const DIALOG_CONTRACTS = {
  'sd.a': {
    id: 'sd.a',
    kind: 'tool',
    commands: ['a_get', 'a_set', 'b_*', 'plugin:dialog|open'],
  },
  'sd.b': {
    id: 'sd.b',
    kind: 'decision',
    commands: [],
  },
} as const;
`;

test('reads the generated contracts', () => {
  assert.deepEqual(contractsOf(MODULE), { 'sd.a': ['a_get', 'a_set', 'b_*', 'plugin:dialog|open'], 'sd.b': [] });
});

test('a declared command no run sent fails unless a reason excuses it; prefixes and plugin commands count by match', () => {
  const contracts = contractsOf(MODULE);
  const seen = { 'sd.a': ['a_get', 'b_list'] };
  assert.deepEqual(dialogCommandProblems({ contracts, seen, unexercised: {}, baseline: 0 }).problems, ['sd.a declares a_set, which no run sent: exercise it, or list it as unexercised with the reason']);
  const ok = dialogCommandProblems({ contracts, seen, unexercised: { 'sd.a': { a_set: 'sent only after a confirmation the e2e cannot answer' } }, baseline: 1 });
  assert.deepEqual(ok, { problems: [], count: 1 });
});

test('the unexercised list is checked and its count may only fall', () => {
  const contracts = contractsOf(MODULE);
  const { problems } = dialogCommandProblems({ contracts, seen: { 'sd.a': ['a_get', 'b_x'] }, unexercised: { 'sd.a': { a_set: ' ' }, 'sd.b': { gone: 'old' } }, baseline: 1 });
  assert.deepEqual(problems, [
    'sd.a declares a_set, which no run sent: exercise it, or list it as unexercised with the reason',
    'unexercised sd.a a_set gives no reason',
    'unexercised names sd.b gone, which no contract declares: remove it',
    '2 unexercised commands, above the baseline of 1: the count may only fall (ADR 0026 D4)',
  ]);
});

test('every dialog must have been opened when the visited list is given', () => {
  const contracts = contractsOf(MODULE);
  const r = dialogCommandProblems({ contracts, seen: { 'sd.a': ['a_get', 'a_set', 'b_x'] }, unexercised: {}, baseline: 0, visited: ['sd.a'] });
  assert.deepEqual(r.problems, ['no e2e opened the dialog sd.b (openEveryDialog)']);
});

test('the command line unions the seen files and reports', () => {
  const files = {
    'c.ts': MODULE,
    's1.json': JSON.stringify({ version: 1, seen: { 'sd.a': ['a_get'] } }),
    's2.json': JSON.stringify({ version: 1, seen: { 'sd.a': ['a_set', 'b_y'] } }),
    'u.json': '{}',
    'n.count': '0\n',
    'v.json': JSON.stringify({ visited: ['sd.a', 'sd.b'] }),
  };
  const lines = [];
  const log = { error: (l) => lines.push(`E ${l}`), log: (l) => lines.push(l) };
  assert.equal(main(['--contracts', 'c.ts', '--seen', 's1.json', 's2.json', '--unexercised', 'u.json', '--baseline', 'n.count', '--visited', 'v.json'], (p) => files[p], log), 0);
  assert.match(lines[0], /0 unexercised, baseline 0/);
  assert.equal(main(['--contracts', 'c.ts', '--seen', 's1.json', '--unexercised', 'u.json', '--baseline', 'n.count'], (p) => files[p], log), 1);
  assert.throws(() => main(['--contracts', 'c.ts'], (p) => files[p], log), /usage/);
});

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { e2ePort, e2ePortOrCiDefault, e2eWorkers, PORT_BLOCKS, PORT_RANGE_START, repoRoot, worktreePort } from './env.ts';

test('the port comes from the environment, else the local default', () => {
  assert.equal(e2ePort('PW_PORT', 4300, { PW_PORT: '4601' }), 4601);
  assert.equal(e2ePort('PW_PORT', 4300, {}, '/wt/a'), worktreePort(4300, '/wt/a'));
  assert.equal(e2ePort('PW_PORT', 4300, { PW_PORT: '' }, '/wt/a'), worktreePort(4300, '/wt/a'));
});

test('CI must name its port, and a port must be one', () => {
  assert.throws(() => e2ePort('PW_PORT', 4300, { CI: 'true' }), /CI: set PW_PORT/);
  assert.equal(e2ePort('PW_PORT', 4300, { CI: 'true', PW_PORT: '4700' }), 4700);
  for (const bad of ['x', '0', '70000', '43.5']) assert.throws(() => e2ePort('PW_PORT', 4300, { PW_PORT: bad }), /is not a port/);
});

test('the local default derives from the worktree: stable, in range, disjoint blocks, one offset per config', () => {
  const a = worktreePort(4300, '/wt/a');
  assert.equal(a, worktreePort(4300, '/wt/a'));
  assert.equal(worktreePort(4352, '/wt/a'), a + 52);
  assert.ok(a >= PORT_RANGE_START && a + 99 < PORT_RANGE_START + PORT_BLOCKS * 100 && PORT_RANGE_START + PORT_BLOCKS * 100 <= 49_152);
  // Two different roots share no port unless their hashes collide: the blocks are 100 wide and the offsets below 100.
  const roots = Array.from({ length: 40 }, (_, i) => `/wt/${i}`);
  const blocks = new Set(roots.map((r) => Math.floor(worktreePort(4300, r) / 100)));
  assert.ok(blocks.size > 25);
  for (const r of roots) assert.equal(Math.floor(worktreePort(4390, r) / 100), Math.floor(worktreePort(4300, r) / 100));
});

test('the apps variant keeps the fixed default on CI instead of failing', () => {
  assert.equal(e2ePortOrCiDefault('PW_PORT', 4352, { CI: 'true' }), 4352);
  assert.equal(e2ePortOrCiDefault('PW_PORT', 4352, { CI: 'true', PW_PORT: '4400' }), 4400);
  assert.equal(e2ePortOrCiDefault('PW_PORT', 4352, {}, '/wt/a'), worktreePort(4352, '/wt/a'));
  assert.throws(() => e2ePortOrCiDefault('PW_PORT', 4352, { PW_PORT: 'x' }), /is not a port/);
});

test('the root of the derivation is the git top level, from any folder below it', () => {
  const top = repoRoot(import.meta.dirname);
  assert.equal(repoRoot(`${import.meta.dirname}/..`), top);
  assert.equal(worktreePort(4300, top), worktreePort(4300, repoRoot(`${import.meta.dirname}/../..`)));
  assert.equal(repoRoot('/'), '/');
});

test('the layout caps a config\'s workers and warns naming both numbers; unset it changes nothing (ER-48)', () => {
  const lines: string[] = [];
  const warn = (l: string) => lines.push(l);
  assert.equal(e2eWorkers(2, {}, warn), 2);
  assert.equal(e2eWorkers(undefined, { PADDOCK_E2E_WORKERS: '' }, warn), undefined);
  assert.deepEqual(lines, []);
  assert.equal(e2eWorkers(2, { PADDOCK_E2E_WORKERS: '5' }, warn), 2, 'within the budget: the config keeps its own number');
  assert.equal(e2eWorkers(undefined, { PADDOCK_E2E_WORKERS: '5' }, warn), 5, 'Playwright\'s own half-the-cores default becomes the layout\'s');
  assert.deepEqual(lines, []);
  // Positive control: a config above the budget is cut and says so.
  assert.equal(e2eWorkers(8, { PADDOCK_E2E_WORKERS: '4' }, warn), 4);
  assert.equal(lines.length, 1);
  assert.match(lines[0]!, /::warning::.*asks for 8 .*gives 4/);
  for (const bad of ['x', '0', '2.5']) assert.throws(() => e2eWorkers(1, { PADDOCK_E2E_WORKERS: bad }, warn), /not a whole number/);
});

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { liveStarts } from '../src/checks/C136-unit-live-feeds.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const TICKS = "starts a live mock feed on the page's timers";

test('C136 bad: a feed that ticks on the page timers, in one line or many, are found with their lines', () => {
  const text = [
    "it('a', async () => {",
    '  const a = new MockBackend({ latency: false, liveMs: 25 });',
    '});',
    "it('b', async () => {",
    '  const b = new MockBackend({',
    '    latency: false,',
    '    dataset: tinyDataset,',
    '    liveMs: 250,',
    '  });',
    '});',
    '',
  ].join('\n');
  assert.deepEqual(liveStarts(text), [
    { line: 2, why: `${TICKS} (\`liveMs: 25\`)` },
    { line: 8, why: `${TICKS} (\`liveMs: 250\`)` },
  ]);
  assert.deepEqual(liveStarts("it('s', () => { const s = synthetic(5000, { liveMs: 25 }); });\n").map((f) => f.line), [1]);
});

test('C136 good: a feed too slow to tick, an options parser, a comment are not found', () => {
  assert.deepEqual(liveStarts("it('x', () => { new MockBackend({ liveMs: 100_000 }); new MockBackend({ liveMs: 100000 }); });\n"), []);
  assert.deepEqual(liveStarts("it('x', () => {\n  expect(parse('?mockLiveMs=300')).toEqual({\n    liveMs: 300,\n  });\n});\n"), []);
  assert.deepEqual(liveStarts("// new MockBackend({ liveMs: 25 })\n"), []);
  assert.deepEqual(liveStarts("it('x', () => { bridge.onLive('summary', fn); });\n"), []);
});

test('C136 is judged per it() block: a clock in the same block or in the shared code covers it, a clock in another block does not', () => {
  const own = "it('a', () => {\n  const c = new ManualClock();\n  new MockBackend({ liveMs: 25, clock: c });\n});\n";
  assert.deepEqual(liveStarts(own), []);
  assert.deepEqual(liveStarts("it('a', () => {\n  vi.useFakeTimers();\n  new MockBackend({ liveMs: 25 });\n});\n"), []);
  const shared = "const make = () => new MockBackend({ liveMs: 15, clock: new ManualClock() });\nit('a', () => {\n  make();\n  new MockBackend({ liveMs: 25 });\n});\n";
  assert.deepEqual(liveStarts(shared), []);
  const other = "it('a', () => {\n  new ManualClock();\n});\nit('b', () => {\n  new MockBackend({ liveMs: 25 });\n});\n";
  assert.deepEqual(liveStarts(other).map((f) => f.line), [5]);
});

test('C136 through the runner: unit specs only, warnings only', async () => {
  const files = {
    'apps/a/src/a.spec.ts': "it('x', () => {\n  new MockBackend({ liveMs: 20 });\n});\n",
    'apps/a/src/held.spec.ts': "it('x', () => {\n  new ManualClock();\n  new MockBackend({ liveMs: 20 });\n});\n",
    'apps/a/e2e/e.spec.ts': "it('x', () => {\n  new MockBackend({ liveMs: 20 });\n});\n",
    'apps/a/src/b.ts': 'const b = new MockBackend({ liveMs: 20 });\n',
  };
  const found = await findingsOf('C136', makeApp(files));
  assert.deepEqual(found.map((f) => [f.file, f.line, f.severity]), [['apps/a/src/a.spec.ts', 2, 'warn']]);
});

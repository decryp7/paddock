import assert from 'node:assert/strict';
import { test } from 'node:test';
import { liveStarts } from '../src/checks/C136-unit-live-feeds.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

test('C136 bad: a feed that ticks on the page timers, and an onLive subscription, are found with their lines', () => {
  const text = "const a = new MockBackend({ latency: false, liveMs: 25 });\nconst b = synthetic(5000, { liveMs: 250 });\nbackend.onLive('summary', fn);\n";
  assert.deepEqual(liveStarts(text), [
    { line: 1, why: "starts a live mock feed on the page's timers (`liveMs: 25`)" },
    { line: 2, why: "starts a live mock feed on the page's timers (`liveMs: 250`)" },
    { line: 3, why: 'subscribes to a live feed (`onLive(`)' },
  ]);
});

test('C136 good: a feed too slow to tick, an options parser, a comment and a file that holds are not found', () => {
  assert.deepEqual(liveStarts("new MockBackend({ liveMs: 100_000 });\nnew MockBackend({ liveMs: 100000 });\n"), []);
  assert.deepEqual(liveStarts("expect(parse('?mockLiveMs=300')).toEqual({\n  liveMs: 300,\n});\n"), []);
  assert.deepEqual(liveStarts("// new MockBackend({ liveMs: 25 })\n * backend.onLive(x)\n"), []);
  for (const holder of ['withLiveHeld(async () => 1)', 'holdLive()', 'holdableTimers(clock)', 'vi.useFakeTimers()', 'new ManualClock()']) {
    assert.deepEqual(liveStarts(`${holder};\nnew MockBackend({ liveMs: 25 });\n`), [], holder);
  }
});

test('C136 through the runner: unit specs only, warnings only', async () => {
  const files = {
    'apps/a/src/a.spec.ts': 'const b = new MockBackend({ liveMs: 20 });\n',
    'apps/a/src/held.spec.ts': 'holdLive();\nconst b = new MockBackend({ liveMs: 20 });\n',
    'apps/a/e2e/e.spec.ts': 'const b = new MockBackend({ liveMs: 20 });\n',
    'apps/a/src/b.ts': 'const b = new MockBackend({ liveMs: 20 });\n',
  };
  const found = await findingsOf('C136', makeApp(files));
  assert.deepEqual(found.map((f) => [f.file, f.line, f.severity]), [['apps/a/src/a.spec.ts', 1, 'warn']]);
});

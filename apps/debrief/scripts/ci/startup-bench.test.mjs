// The start-up benchmark's parsing and budget check, and one run against a stub app (a shell script that logs a
// "Startup timings" line and waits to be stopped). The stub needs a POSIX shell; skipped on Windows.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { collectSamples, evaluate, loadOk, MARKS, median, notMeasuredMessage, parseTimingsLine, readLoad, runOnce } from './startup-bench.mjs';

const fields = { WindowCreatedMs: 100, SetupDoneMs: 300, PageLoadMs: 600, FirstPaintMs: 700, WindowReadyMs: 710, WorkspaceLoadedMs: 720, FirstInteractiveMs: 730 };
const line = (extra = {}) => JSON.stringify({ '@t': '2026-10-24T00:00:00Z', '@m': 'Startup timings: …', Steps: 'app state=20', ...fields, ...extra });

test('reads the marks of the "Startup timings" line and ignores other lines', () => {
  const t = parseTimingsLine(line());
  assert.equal(t.marks.firstInteractive, 730);
  assert.equal(t.marks.windowCreated, 100);
  assert.equal(t.steps, 'app state=20');
  assert.equal(parseTimingsLine('{"@m":"Window main ready"}'), null);
  assert.equal(parseTimingsLine('not json'), null);
  assert.equal(parseTimingsLine(line({ FirstPaintMs: 'soon' })), null, 'a line without every mark is not counted');
  assert.equal(MARKS.length, 7);
});

test('medians per mark and the marks over budget', () => {
  assert.equal(median([3, 1, 2]), 2);
  assert.equal(median([4, 1, 2, 3]), 2.5);
  const runs = [700, 800, 900].map((ms) => ({ marks: { ...parseTimingsLine(line()).marks, firstInteractive: ms } }));
  const { medians, over } = evaluate(runs, { firstInteractive: 750, setupDone: 400 });
  assert.equal(medians.firstInteractive, 800);
  assert.deepEqual(over, [{ mark: 'firstInteractive', median: 800, budget: 750 }]);
  assert.deepEqual(evaluate(runs, undefined).over, []);
});

test('runs a start, reads its line and stops it', { skip: process.platform === 'win32' }, async () => {
  const dir = mkdtempSync(join(tmpdir(), 'debrief-bench-stub-'));
  try {
    const exe = join(dir, 'debrief-app');
    writeFileSync(exe, `#!/bin/sh\necho 'starting'\necho '${line()}'\nexec sleep 30\n`);
    chmodSync(exe, 0o755);
    const t = await runOnce(exe, dir, 10_000);
    assert.equal(t.marks.setupDone, 300);
    const quits = join(dir, 'quits');
    writeFileSync(quits, '#!/bin/sh\necho no timings\nexit 3\n');
    chmodSync(quits, 0o755);
    await assert.rejects(runOnce(quits, dir, 10_000), /exited \(code 3\)/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// The load precondition (ER-33): load readings are injected, nothing is launched, nothing waits.
const sample = (ms) => ({ marks: { ...parseTimingsLine(line()).marks, setupDone: ms }, steps: '' });
const collect = (loads, runsMs, over = {}) => {
  const l = [...loads];
  const r = [...runsMs];
  let t = 0;
  return collectSamples({
    want: 3,
    max: 12,
    maxAttempts: 9,
    waitMs: 1000,
    capMs: 100_000,
    measure: async () => sample(r.shift()),
    load: () => (l.length ? l.shift() : 1),
    sleep: async (ms) => void (t += ms),
    now: () => t,
    ...over,
  });
};

test('a sample counts only when the load before and after is within the threshold', () => {
  assert.equal(loadOk(3, 4, 12), true);
  assert.equal(loadOk(12, 12, 12), true);
  assert.equal(loadOk(13, 4, 12), false);
  assert.equal(loadOk(3, 12.1, 12), false);
  assert.equal(loadOk(null, null, 12), false, 'no load average (Windows): not verified, not a pass');
  assert.equal(loadOk(null, null, 12, true), true, 'unless report-only');
  assert.equal(readLoad('win32', () => [0, 0, 0]), null);
  assert.equal(readLoad('darwin', () => [2.5, 1, 1]), 2.5);
});

test('quiet host: every start counts and carries its load readings', async () => {
  const c = await collect([2, 2.5, 3, 3, 4, 4], [230, 232, 240]);
  assert.equal(c.measured, true);
  assert.deepEqual(c.samples.map((s) => [s.loadBefore, s.loadAfter]), [[2, 2.5], [3, 3], [4, 4]]);
  assert.equal(c.discarded.length, 0);
});

test('a start over the load threshold is discarded and replaced by a later quiet one', async () => {
  // attempt 1: busy before (no start); attempt 2: busy after (start discarded); then three quiet ones
  const c = await collect([20, 3, 15, 2, 2, 2, 2, 2, 2], [434, 230, 231, 232]);
  assert.equal(c.measured, true);
  assert.deepEqual(c.samples.map((s) => s.marks.setupDone), [230, 231, 232]);
  assert.deepEqual(c.discarded.map((d) => d.started), [false, true]);
  assert.equal(c.peakLoad, 20);
});

test('too few quiet starts within the attempts is NOT MEASURED, with the load seen in the message', async () => {
  const c = await collect(Array(20).fill(25), []);
  assert.equal(c.measured, false);
  assert.equal(c.samples.length, 0);
  assert.equal(c.attempts, 9, 'bounded by maxAttempts');
  const msg = notMeasuredMessage(c, 3, 12);
  assert.match(msg, /^::error::start-up benchmark NOT MEASURED/);
  assert.match(msg, /peak 1-minute load 25\.0/);
  assert.match(msg, /Next step/);
});

test('unavailable load is NOT MEASURED at once, and counts only when report-only', async () => {
  const c = await collect(Array(20).fill(null), [230, 231, 232]);
  assert.equal(c.measured, false);
  assert.equal(c.unverified, true);
  assert.equal(c.attempts, 1);
  assert.match(notMeasuredMessage(c, 3, 12), /unavailable.*not verified/);
  const r = await collect(Array(20).fill(null), [230, 231, 232], { allowUnknown: true });
  assert.equal(r.measured, true);
});

test('the total cap also stops the collection', async () => {
  const c = await collect(Array(20).fill(25), [], { capMs: 2500 });
  assert.equal(c.attempts, 3);
  assert.equal(c.measured, false);
});

test('no start and no sleep begins when the cap would be passed', async () => {
  let started = 0;
  const c = await collect(Array(20).fill(25), [], { capMs: 1000, measure: async () => void started++ });
  assert.equal(started, 0);
  assert.equal(c.attempts, 1, 'the wait would pass the cap, so it stops instead of sleeping');
});

test('valid samples over budget still fail the budget check', async () => {
  const c = await collect([2, 2, 2, 2, 2, 2], [500, 520, 540]);
  assert.equal(c.measured, true);
  assert.deepEqual(evaluate(c.samples, { setupDone: 400 }).over, [{ mark: 'setupDone', median: 520, budget: 400 }]);
});

// Wiring: the real script, a fake app, a load limit nothing can meet (--max-load 0). Exit code, report-only and JSON.
test('script: exit 6 and a ::error:: when the load is over the limit; report-only warns; JSON has the load', { skip: process.platform === 'win32' }, async () => {
  const { spawnSync } = await import('node:child_process');
  const { readFileSync } = await import('node:fs');
  const { fileURLToPath } = await import('node:url');
  const dir = mkdtempSync(join(tmpdir(), 'debrief-bench-wire-'));
  try {
    const exe = join(dir, 'debrief-app');
    writeFileSync(exe, `#!/bin/sh\necho '${line()}'\nexec sleep 30\n`);
    chmodSync(exe, 0o755);
    const script = fileURLToPath(new URL('./startup-bench.mjs', import.meta.url));
    const go = (...extra) => {
      const out = join(dir, `out-${extra.length}.json`);
      const r = spawnSync(process.execPath, [script, exe, '--runs', '2', '--os', 'macos', '--max-load', '-1', '--wait-s', '0', '--max-attempts', '2', '--out', out, ...extra], { encoding: 'utf8', timeout: 60_000 });
      return { r, json: JSON.parse(readFileSync(out, 'utf8')) };
    };
    const strict = go();
    assert.equal(strict.r.status, 6);
    assert.match(strict.r.stderr, /::error::start-up benchmark NOT MEASURED/);
    assert.match(strict.r.stdout, /load precondition: 1-min load <= -1 \(source: flag\)/);
    assert.equal(strict.json.load.measured, false);
    assert.equal(strict.json.load.max, -1);
    const report = go('--report-only');
    assert.equal(report.r.status, 0);
    assert.match(report.r.stderr, /::warning::start-up benchmark NOT MEASURED/);
    assert.doesNotMatch(report.r.stderr, /::error::/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

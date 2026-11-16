// The verdict logic of a benchmark on a busy host, and the parsers of its readers. Nothing is launched, nothing waits.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  annotations,
  budgetMs,
  iqrSpread,
  quantile,
  EXIT_NOT_MEASURED,
  EXIT_OVER,
  isTagRun,
  median,
  parseCpuTime,
  parseTopIdle,
  procStatIdle,
  readCpuIdle,
  readTreeCpuMs,
  referenceStats,
  referenceWork,
  treeCpuMs,
  verdict,
  wallScale,
} from './busy-host.mjs';

const T = (baselineMs, noiseMs) => ({ baselineMs, noiseMs });
const thresholds = { cpu: T(400, 60), marks: { setupDone: T(250, 30), firstInteractive: T(900, 100) }, samples: { maxSpread: 0.3 } };
const steady = { reps: [100, 101, 99, 100, 102], baselineMs: 100, maxSpread: 0.1, normaliseWall: false };
const input = (o = {}) => ({
  cpuMsSamples: [410, 420, 415, 405, 430],
  wallSamples: [900, 910, 905, 920, 915].map((ms) => ({ setupDone: 240, firstInteractive: ms })),
  reference: steady,
  thresholds,
  tag: false,
  ...o,
});

test('measured and under every threshold passes, with the numbers in the result', () => {
  const v = verdict(input());
  assert.equal(v.status, 'pass');
  assert.equal(v.exitCode, 0);
  assert.equal(v.level, null);
  assert.equal(v.cpuMedian, 415);
  assert.equal(v.scaledWall.firstInteractive, 910, 'wall time is left as it is unless normalising is on');
  assert.ok(Math.abs(v.ratio - 9.1) < 1e-9, 'start-up as a ratio to the reference workload');
  assert.ok(v.sampleSpread < 0.05);
});

test('measured and over the CPU threshold fails, on a tag and on a train alike', () => {
  for (const tag of [false, true]) {
    const v = verdict(input({ cpuMsSamples: [500, 510, 505, 520, 515], tag }));
    assert.equal(v.status, 'fail');
    assert.equal(v.exitCode, EXIT_OVER);
    assert.deepEqual(v.over, [{ kind: 'cpu', median: 510, budget: 460 }]);
    assert.match(annotations(v, { name: 'start-up benchmark', tag, os: 'macos' })[0], /^::error::.*CPU.*510 ms.*460 ms/);
  }
});

test('measured and over a wall threshold fails, naming the mark', () => {
  const v = verdict(input({ wallSamples: [1100, 1110, 1105, 1120, 1115].map((ms) => ({ setupDone: 240, firstInteractive: ms })) }));
  assert.equal(v.status, 'fail');
  assert.deepEqual(v.over.map((o) => [o.kind, o.mark, o.budget]), [['wall', 'firstInteractive', 1000]]);
  assert.match(annotations(v, { name: 'start-up', tag: false, os: 'macos' })[0], /^::error::start-up firstInteractive: median 1110 ms exceeds the 1000 ms threshold/);
});

test('a threshold is the recorded baseline plus the noise margin', () => {
  assert.equal(budgetMs(T(400, 60)), 460);
  assert.equal(budgetMs({ baselineMs: 400 }), null);
  assert.equal(budgetMs(undefined), null);
  assert.equal(verdict(input({ cpuMsSamples: [460, 460, 460] })).status, 'pass', 'exactly at the threshold is not over');
  assert.equal(verdict(input({ cpuMsSamples: [461, 461, 461] })).status, 'fail');
});

test('busy host, measured on the Mac: a slow reference does not move the verdict, so a regression is not hidden', () => {
  // The reference ran 3 times slower than its baseline (a CPU-bound loop under yes loops); the start did not move.
  const busy = { ...steady, reps: [300, 310, 290, 305, 295], normaliseWall: false };
  const regressed = [1100, 1110, 1105, 1120, 1115].map((ms) => ({ setupDone: 240, firstInteractive: ms }));
  assert.equal(verdict(input({ reference: busy, wallSamples: regressed })).status, 'fail', 'a 20% slower start fails even on a busy host');
  assert.equal(verdict(input({ reference: busy })).status, 'pass');
});

test('with normalising on, a slow reference takes the host slowdown off the wall time', () => {
  const busy = { reps: [180, 190, 200, 185, 195], baselineMs: 100, maxSpread: 0.25, normaliseWall: true };
  const slow = [1700, 1750, 1800, 1720, 1780].map((ms) => ({ setupDone: 240, firstInteractive: ms }));
  const ok = verdict(input({ reference: busy, wallSamples: slow }));
  assert.equal(ok.status, 'pass');
  assert.ok(Math.abs(ok.scale - 100 / 190) < 1e-9);
  const regressed = verdict(input({ reference: busy, wallSamples: [2500, 2600, 2550, 2520, 2580].map((ms) => ({ setupDone: 240, firstInteractive: ms })) }));
  assert.equal(regressed.status, 'fail');
});

test('a host faster than the baseline never scales the wall time up', () => {
  assert.equal(wallScale(100, 50), 1);
  assert.equal(wallScale(100, 200), 0.5);
  assert.equal(wallScale(undefined, 200), 1);
  assert.equal(wallScale(100, NaN), 1);
});

test('starts that are not steady are NOT MEASURED: a warning and exit 0 on a train, an error and exit 6 on a tag', () => {
  const wild = { cpuMsSamples: [300, 900, 400, 1200, 350] };
  const train = verdict(input(wild));
  assert.equal(train.status, 'not-measured');
  assert.equal(train.exitCode, 0, 'a train run stays green');
  assert.equal(train.level, 'warning');
  const [w] = annotations(train, { name: 'start-up benchmark', tag: false, os: 'macos' });
  assert.match(w, /^::warning::start-up benchmark NOT MEASURED/);
  assert.match(w, /spread \d+% is over the 30% limit/);
  assert.match(w, /Next step/);
  const tag = verdict(input({ ...wild, tag: true }));
  assert.equal(tag.status, 'not-measured');
  assert.equal(tag.exitCode, EXIT_NOT_MEASURED);
  assert.equal(tag.level, 'error');
  assert.match(annotations(tag, { name: 'start-up benchmark', tag: true, os: 'macos' })[0], /^::error::.*NOT MEASURED.*ER-33.*Next step/);
});

test('starts whose median is 0 have no spread: NOT MEASURED, not a pass', () => {
  const v = verdict(input({ cpuMsSamples: [0, 0, 0] }));
  assert.equal(v.status, 'not-measured');
  assert.match(v.notes[0], /cannot be computed/);
});

test('one outlier among five does not make the starts unsteady (the middle half is judged)', () => {
  const v = verdict(input({ cpuMsSamples: [490, 490, 490, 520, 1050].map((x) => x - 80) }));
  assert.equal(v.status, 'pass');
  assert.equal(v.cpuMedian, 410);
});

test('fewer than 3 starts, or no recorded spread limit, is NOT MEASURED too', () => {
  const few = verdict(input({ cpuMsSamples: [410, 420], wallSamples: [{ firstInteractive: 900 }, { firstInteractive: 910 }] }));
  assert.equal(few.status, 'not-measured');
  assert.match(few.notes[0], /only 2 usable starts/);
  const none = verdict(input({ thresholds: { ...thresholds, samples: {} } }));
  assert.equal(none.status, 'not-measured');
  assert.match(none.notes[0], /no spread limit/);
});

test('with normalising on, an unstable or missing reference is NOT MEASURED, with it off it is ignored', () => {
  const wild = { reps: [100, 300, 90, 250, 120], baselineMs: 100, maxSpread: 0.1, normaliseWall: true };
  const on = verdict(input({ reference: wild }));
  assert.equal(on.status, 'not-measured');
  assert.match(on.notes[0], /reference workload's spread \d+% is over the 10% limit/);
  assert.equal(verdict(input({ reference: { ...wild, normaliseWall: false } })).status, 'pass');
  assert.equal(verdict(input({ reference: { reps: [100], baselineMs: 100, maxSpread: 0.1, normaliseWall: true } })).status, 'not-measured');
  const noLimit = verdict(input({ reference: { reps: [100, 100], baselineMs: 100, normaliseWall: true } }));
  assert.match(noLimit.notes[0], /no reference spread limit/);
  assert.equal(verdict(input({ reference: undefined })).status, 'pass', 'no reference at all is fine when the wall time is not normalised');
});

test('report-only never fails and never errors, tag or not', () => {
  const over = verdict(input({ cpuMsSamples: [900, 900, 900], reportOnly: true, tag: true }));
  assert.equal(over.status, 'pass');
  assert.equal(over.exitCode, 0);
  const wild = verdict(input({ cpuMsSamples: [300, 900, 400, 1200, 350], reportOnly: true, tag: true }));
  assert.equal(wild.exitCode, 0);
  assert.equal(wild.level, 'warning');
});

test('no thresholds for an OS: numbers only, a pass', () => {
  const v = verdict(input({ thresholds: undefined, reportOnly: true }));
  assert.equal(v.status, 'pass');
  assert.match(v.notes[0], /no thresholds/);
});

test('an OS with no readable tree CPU judges the wall time of the starts and says so', () => {
  const v = verdict(input({ cpuMsSamples: [null, null, null] }));
  assert.equal(v.status, 'pass');
  assert.match(v.notes[0], /not readable/);
  const bad = verdict(input({ cpuMsSamples: [null, null, null], wallSamples: [1500, 1510, 1520].map((ms) => ({ setupDone: 240, firstInteractive: ms })) }));
  assert.equal(bad.status, 'fail');
});

test('a tag run is read from the environment Gitea Actions sets', () => {
  assert.equal(isTagRun({ GITHUB_REF_TYPE: 'tag' }), true);
  assert.equal(isTagRun({ GITHUB_REF: 'refs/tags/debrief-v1.2.0' }), true);
  assert.equal(isTagRun({ GITHUB_REF_TYPE: 'branch', GITHUB_REF: 'refs/heads/train/2026-11-15-13' }), false);
  assert.equal(isTagRun({}), false);
});

test('reference statistics: median and (max - min) / median, ignoring unusable readings', () => {
  const s = referenceStats([100, 110, NaN, 90, -1]);
  assert.equal(s.n, 3);
  assert.equal(s.median, 100);
  assert.equal(s.spread, 0.2);
  assert.equal(referenceStats([]).n, 0);
  assert.equal(median([3, 1, 2]), 2);
  assert.equal(quantile([1, 2, 3, 4, 5], 0.25), 2);
  assert.equal(iqrSpread([490, 490, 490, 520, 1050]), 30 / 490, 'the outlier does not move it');
  assert.ok(Number.isNaN(median([])));
});

test('ps cputime strings and the tree sum', () => {
  assert.equal(parseCpuTime('0:01.50'), 1500);
  assert.equal(parseCpuTime('2:03.25'), 123250);
  assert.equal(parseCpuTime('1:02:03.00'), 3723000);
  assert.equal(parseCpuTime('1-00:00:01'), 86401000);
  assert.ok(Number.isNaN(parseCpuTime('soon')));
  const ps = ['  100     1   0:01.00', '  200   100   0:02.50', '  300   200   0:00.50', '  400     1   9:99.00', '  500   999   0:07.00'].join('\n');
  assert.equal(treeCpuMs(ps, 100), 4000, 'the root and its descendants, not a sibling tree');
  assert.equal(treeCpuMs(ps, 300), 500);
  assert.equal(treeCpuMs(ps, 777), null, 'a root that is gone has no reading');
  assert.equal(treeCpuMs('  100 1 garbage\n', 100), null, 'an unreadable time is no reading, not zero');
});

test('the readers run the OS tools only where they exist', () => {
  const calls = [];
  const run = (cmd, args) => (calls.push([cmd, ...args]), { status: 0, stdout: '  42  1  0:00.30\n' });
  assert.equal(readTreeCpuMs(42, 'darwin', run), 300);
  assert.equal(readTreeCpuMs(42, 'linux', run), null, 'ps cputime has a 1 s resolution there: no reading');
  assert.equal(readTreeCpuMs(42, 'win32', run), null);
  assert.deepEqual(calls.map((c) => c[0]), ['ps']);
  const top = 'CPU usage: 9.0% user, 9.0% sys, 82.0% idle\nCPU usage: 3.1% user, 4.2% sys, 92.7% idle\n';
  assert.equal(readCpuIdle('darwin', () => ({ status: 0, stdout: top })), 92.7, 'the second sample is the one measured over the interval');
  assert.equal(readCpuIdle('darwin', () => ({ status: 1, stdout: '' })), null);
  assert.equal(readCpuIdle('win32'), null);
  assert.equal(parseTopIdle('no cpu line'), null);
  assert.equal(procStatIdle('cpu 10 0 10 80 0 0 0 0', 'cpu 20 0 20 160 0 0 0 0'), 80);
  assert.equal(procStatIdle('cpu 1 2', 'cpu 1 2'), null);
});

test('the reference workload is deterministic', () => {
  assert.equal(referenceWork(10_000), referenceWork(10_000));
  assert.notEqual(referenceWork(10_000), referenceWork(10_001));
});

// The verdict logic of a benchmark on a busy host, and the parsers of its readers. Nothing is launched, nothing waits.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  annotations,
  canReadTreeCpu,
  budgetMs,
  ceilingMs,
  hostSummary,
  medianIdleOf,
  iqrSpread,
  quantile,
  EXIT_NOT_MEASURED,
  EXIT_OVER,
  isTagRun,
  median,
  parseCpuTime,
  parseIostatIdle,
  procStatIdle,
  readCpuIdle,
  readTreeCpuMs,
  referenceStats,
  referenceWork,
  treeCpuMs,
  verdict,
} from './busy-host.mjs';

const T = (baselineMs, noiseMs) => ({ baselineMs, noiseMs });
// cpu threshold 460; firstInteractive: noise threshold 1000, ceiling 3 x 900 = 2700
const thresholds = { cpu: T(400, 60), marks: { setupDone: T(250, 30), firstInteractive: T(900, 100) }, wallCeilingFactor: 3 };
const wall = (...ms) => ms.map((x) => ({ setupDone: 240, firstInteractive: x }));
const input = (o = {}) => ({
  cpuMsSamples: [410, 420, 415, 405, 430],
  wallSamples: wall(900, 910, 905, 920, 915),
  reference: { reps: [100, 101, 99, 100, 102] },
  thresholds,
  ...o,
});

test('measured and under every threshold passes, with the numbers as notes', () => {
  const v = verdict(input({ idle: [{ idleBefore: 80, idleAfter: 70, loadBefore: 3, loadAfter: 4 }] }));
  assert.equal(v.status, 'pass');
  assert.equal(v.exitCode, 0);
  assert.equal(v.level, null);
  assert.equal(v.cpuMedian, 415);
  assert.ok(Math.abs(v.ratio - 9.1) < 1e-9, 'start-up as a ratio to the reference workload');
  assert.match(v.notes.join('\n'), /9\.1 times the reference workload/);
  assert.match(v.notes.join('\n'), /CPU idle 70 to 80%, 1-minute load 3\.0 to 4\.0/);
});

test('a busy host passes with a note: slow wall time under the ceiling, a wild spread, a slow reference, no idle', () => {
  const v = verdict({
    cpuMsSamples: [410, 700, 415, 900, 430], // spread, but the median is under the threshold
    wallSamples: wall(1500, 1700, 1600, 2400, 1650), // over the noise threshold, under the ceiling
    reference: { reps: [300, 500, 280, 700, 350] },
    thresholds,
    idle: [{ idleBefore: 0, idleAfter: 1, loadBefore: 49.4, loadAfter: 50 }],
    tag: true,
  });
  assert.equal(v.status, 'pass', 'a busy host is measured, on a tag too');
  assert.equal(v.exitCode, 0);
  const notes = v.notes.join('\n');
  assert.match(notes, /firstInteractive median 1650 ms is over its 1000 ms noise threshold, under the 2700 ms ceiling: a note/);
  assert.match(notes, /the starts' own spread .* is \d+%/);
  assert.match(notes, /CPU idle 0 to 1%, 1-minute load 49\.4 to 50\.0/);
  assert.doesNotMatch(annotations(v, { name: 'b', os: 'macos' }).join(''), /./, 'no annotation for a pass');
});

test('the CPU time over its threshold fails, on a busy host and a quiet one', () => {
  const v = verdict(input({ cpuMsSamples: [500, 510, 505, 520, 515], idle: [{ idleBefore: 0, idleAfter: 0 }] }));
  assert.equal(v.status, 'fail');
  assert.equal(v.exitCode, EXIT_OVER);
  assert.deepEqual(v.over, [{ kind: 'cpu', median: 510, budget: 460, quiet: false }]);
  assert.match(annotations(v, { name: 'start-up benchmark', os: 'macos' })[0], /^::error::.*CPU.*510 ms.*460 ms/);
});

// Tight when quiet: the quiet limit is 400 + 20 = 420 (minIdle 50); the busy limit is 460.
const withQuiet = { ...thresholds, cpu: { ...T(400, 60), quiet: { ...T(350, 70), minIdle: 50 } } };
const hostIdle = (...v) => v.map((x) => ({ idleBefore: x, idleAfter: x }));

test('a quiet host over the tight CPU limit fails, though it is under the busy limit', () => {
  const v = verdict(input({ thresholds: withQuiet, cpuMsSamples: [440, 445, 450, 440, 442], idle: hostIdle(80, 70, 60, 90, 75) }));
  assert.equal(v.status, 'fail');
  assert.equal(v.quiet, true);
  assert.deepEqual(v.over, [{ kind: 'cpu', median: 442, budget: 420, quiet: true }]);
  assert.match(annotations(v, { name: 'start-up', os: 'macos' })[0], /exceeds the 420 ms quiet-host threshold.*the host was quiet/);
});

test('a busy host between the tight and the busy limit passes with a note naming both and the idle seen', () => {
  const v = verdict(input({ thresholds: withQuiet, cpuMsSamples: [440, 445, 450, 440, 442], idle: hostIdle(40, 30, 10, 20, 15) }));
  assert.equal(v.status, 'pass');
  assert.equal(v.quiet, false);
  assert.match(v.notes.join('\n'), /442 ms is over the quiet-host limit 420 ms, under the busy-host limit 460 ms: a note.*median CPU idle 20%.*50% or more/);
});

test('a busy host over the busy limit still fails, with the busy-host wording', () => {
  const v = verdict(input({ thresholds: withQuiet, cpuMsSamples: [500, 510, 505, 520, 515], idle: hostIdle(0, 0, 0) }));
  assert.equal(v.status, 'fail');
  assert.match(annotations(v, { name: 'b', os: 'macos' })[0], /460 ms busy-host threshold/);
});

test('the idle cutoff is on the median reading: exactly minIdle is quiet, below is busy, no reading is busy', () => {
  const cpu = [440, 440, 440];
  const q = (idle) => verdict(input({ thresholds: withQuiet, cpuMsSamples: cpu, idle }));
  assert.equal(q(hostIdle(50, 50)).status, 'fail', 'at the cutoff: quiet');
  assert.equal(q(hostIdle(49.9, 49.9)).status, 'pass', 'just below: busy');
  assert.equal(q(hostIdle(10, 90, 90, 90, 90)).status, 'fail', 'one dip among high readings does not make the host busy');
  assert.equal(q(hostIdle(90, 10, 10, 10, 10)).status, 'pass', 'one high reading among low ones does not make it quiet');
  assert.equal(q([{ idleBefore: 90, idleAfter: 49 }, { idleBefore: 49, idleAfter: 49 }]).status, 'pass', 'before and after are both readings');
  assert.equal(q(undefined).status, 'pass', 'idle not read: the busy limit');
  assert.equal(q([{ idleBefore: null, idleAfter: null }]).status, 'pass');
  assert.equal(verdict(input({ thresholds: withQuiet, cpuMsSamples: [420, 420, 420], idle: hostIdle(90) })).status, 'pass', 'exactly at the tight limit is not over');
  assert.equal(verdict(input({ thresholds: withQuiet, cpuMsSamples: [421, 421, 421], idle: hostIdle(90) })).status, 'fail');
  assert.equal(medianIdleOf([{ idleBefore: 5, idleAfter: 7 }, { idleBefore: 3 }]), 5);
  assert.ok(Number.isNaN(medianIdleOf([])));
  assert.equal(verdict(input({ thresholds, cpuMsSamples: cpu, idle: hostIdle(90) })).quiet, false, 'no quiet limit recorded: only the busy one');
});

test('a threshold is the recorded baseline plus the noise margin; the ceiling is a factor of the baseline', () => {
  assert.equal(budgetMs(T(400, 60)), 460);
  assert.equal(budgetMs({ baselineMs: 400 }), null);
  assert.equal(ceilingMs(T(900, 100), 3), 2700);
  assert.equal(ceilingMs(T(900, 100), undefined), null);
  assert.equal(verdict(input({ cpuMsSamples: [460, 460, 460] })).status, 'pass', 'exactly at the threshold is not over');
  assert.equal(verdict(input({ cpuMsSamples: [461, 461, 461] })).status, 'fail');
});

test('a wall mark over its ceiling fails, naming the mark', () => {
  const v = verdict(input({ wallSamples: wall(2800, 2900, 2850, 3000, 2950) }));
  assert.equal(v.status, 'fail');
  assert.deepEqual(v.over.map((o) => [o.kind, o.mark, o.budget]), [['wall', 'firstInteractive', 2700]]);
  assert.match(annotations(v, { name: 'start-up', os: 'macos' })[0], /^::error::start-up firstInteractive: median 2900 ms exceeds the 2700 ms ceiling/);
  assert.equal(verdict(input({ wallSamples: wall(2700, 2700, 2700) })).status, 'pass', 'exactly at the ceiling is not over');
});

test('no timings is NOT MEASURED, an error and exit 6 on a train and a tag alike', () => {
  for (const tag of [false, true]) {
    const v = verdict(input({ cpuMsSamples: [], wallSamples: [], tag }));
    assert.equal(v.status, 'not-measured');
    assert.equal(v.exitCode, EXIT_NOT_MEASURED);
    assert.equal(v.level, 'error');
    assert.match(annotations(v, { name: 'start-up benchmark', os: 'macos' })[0], /^::error::start-up benchmark NOT MEASURED \(macos\): the app produced no start-up timings.*ER-33.*Next step/);
  }
  assert.equal(verdict(input({ cpuMsSamples: [null], wallSamples: [{}] })).status, 'not-measured', 'samples without a single number are no timings');
});

test('one or two starts are still measured, with a note that there is no spread', () => {
  const v = verdict(input({ cpuMsSamples: [410, 420], wallSamples: wall(900, 910) }));
  assert.equal(v.status, 'pass');
  assert.match(v.notes.join('\n'), /fewer than 3 starts: no spread/);
  assert.equal(verdict(input({ cpuMsSamples: [900], wallSamples: wall(900) })).status, 'fail', 'and still fail over the CPU threshold');
});

test('report-only never fails and never errors', () => {
  const over = verdict(input({ cpuMsSamples: [900, 900, 900], reportOnly: true }));
  assert.equal(over.status, 'pass');
  assert.equal(over.exitCode, 0);
  const none = verdict(input({ cpuMsSamples: [], wallSamples: [], reportOnly: true }));
  assert.equal(none.exitCode, 0);
  assert.equal(none.level, 'warning');
});

test('no thresholds for an OS: numbers only, a pass', () => {
  const v = verdict(input({ thresholds: undefined, reportOnly: true }));
  assert.equal(v.status, 'pass');
  assert.match(v.notes.join('\n'), /no thresholds/);
});

test('an OS with no readable tree CPU judges the wall time of the starts and says so', () => {
  const v = verdict(input({ cpuMsSamples: [null, null, null] }));
  assert.equal(v.status, 'pass');
  assert.match(v.notes.join('\n'), /not readable/);
  const bad = verdict(input({ cpuMsSamples: [null, null, null], wallSamples: wall(2800, 2810, 2820) }));
  assert.equal(bad.status, 'fail');
});

test('a missing CPU time where the OS can read it is a broken check (ER-33), not a note', () => {
  const v = verdict(input({ cpuMsSamples: [null, null, null], cpuExpected: true }));
  assert.equal(v.status, 'check-broken', 'the step summary shows what the annotation says');
  assert.equal(v.exitCode, EXIT_NOT_MEASURED);
  assert.deepEqual(v.over, [{ kind: 'broken-cpu' }]);
  assert.match(annotations(v, { name: 'b', os: 'macos' })[0], /^::error::b check broken \(macos\).*could not be read.*ER-33/);
  assert.equal(verdict(input({ cpuMsSamples: [null, null, null], cpuExpected: false })).status, 'pass', 'where the OS has no reader it is the note');
  assert.equal(verdict(input({ cpuMsSamples: [null, null, null], cpuExpected: true, thresholds: { marks: thresholds.marks, wallCeilingFactor: 3 } })).status, 'pass', 'no CPU limit defined: nothing to read');
  assert.equal(verdict(input({ cpuMsSamples: [null, null, null], cpuExpected: true, reportOnly: true })).status, 'pass', 'report-only never fails');
  assert.equal(canReadTreeCpu('darwin'), true);
  assert.equal(canReadTreeCpu('linux'), false);
});

test('a start with no timings among good ones fails the run; with no good start it is NOT MEASURED, naming the cause', () => {
  const mixed = verdict(input({ failures: [{ start: 3, message: 'Debrief exited (code 3)' }] }));
  assert.equal(mixed.status, 'fail');
  assert.equal(mixed.exitCode, EXIT_OVER);
  assert.match(annotations(mixed, { name: 'b', os: 'macos' })[0], /^::error::b: start 3 gave no timings: a broken start \(Debrief exited \(code 3\)\)/);
  assert.equal(verdict(input({ failures: [{ start: 3, message: 'x' }], reportOnly: true })).status, 'pass');
  const none = verdict(input({ cpuMsSamples: [], wallSamples: [], failures: [{ start: 1, message: 'no line within 60 s' }, { start: 2, message: 'exited' }] }));
  assert.equal(none.status, 'not-measured');
  assert.match(none.notes[0], /start 1: no line within 60 s; start 2: exited/);
});

test('annotations take the app\'s next steps and have neutral defaults', () => {
  const v = verdict(input({ cpuMsSamples: [900, 900, 900] }));
  assert.match(annotations(v, { name: 'b', os: 'macos' })[0], /Next step: compare the steps of the start/);
  assert.match(annotations(v, { name: 'b', os: 'macos', nextSteps: { regression: 'Next step: open the profile.' } })[0], /Next step: open the profile\.$/);
  const none = verdict(input({ cpuMsSamples: [], wallSamples: [] }));
  assert.doesNotMatch(annotations(none, { name: 'b', os: 'x' })[0], /Debrief/);
  assert.match(annotations(none, { name: 'b', os: 'x', nextSteps: { notMeasured: 'Next step: read app.log.' } })[0], /read app\.log\.$/);
});

test('host summary: ranges of idle and load, or nothing', () => {
  assert.equal(hostSummary([]), null);
  assert.equal(hostSummary([{ idleBefore: null }]), null);
  assert.match(hostSummary([{ idleBefore: 10, idleAfter: 90 }]), /CPU idle 10 to 90%, load n\/a/);
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
  assert.deepEqual(calls[0], ['ps', '-S', '-axo', 'pid=,ppid=,cputime='], 'exited children are summed into their parent (-S)');
  assert.equal(readTreeCpuMs(42, 'linux', run), null, 'ps cputime has a 1 s resolution there: no reading');
  assert.equal(readTreeCpuMs(42, 'win32', run), null);
  assert.equal(calls.length, 1, 'only darwin runs ps');
  const top = '      cpu    load average\n us sy id   1m   5m   15m\n 34 13 53  68.69 57.08 40.94\n 79 21  0  68.69 57.08 40.94\n';
  assert.equal(readCpuIdle('darwin', () => ({ status: 0, stdout: top })), 0, 'the second sample is the one measured over the interval');
  assert.equal(readCpuIdle('darwin', () => ({ status: 1, stdout: '' })), null);
  assert.equal(readCpuIdle('win32'), null);
  assert.equal(parseIostatIdle('no cpu line'), null);
  assert.equal(parseIostatIdle(' 34 13 53  1 2 3\n'), null, 'one row is since boot, not an interval');
  assert.equal(procStatIdle('cpu 10 0 10 80 0 0 0 0', 'cpu 20 0 20 160 0 0 0 0'), 80);
  assert.equal(procStatIdle('cpu 1 2', 'cpu 1 2'), null);
});

test('the reference workload is deterministic', () => {
  assert.equal(referenceWork(10_000), referenceWork(10_000));
  assert.notEqual(referenceWork(10_000), referenceWork(10_001));
});

// The start-up benchmark's parsing and budget check, and one run against a stub app (a shell script that logs a
// "Startup timings" line and waits to be stopped). The stub needs a POSIX shell; skipped on Windows.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { collectSamples, evaluate, MARKS, median, parseTimingsLine, runOnce } from './startup-bench.mjs';

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

// Collecting samples: no start is dropped for a busy host; each carries the host's CPU idle and load around it.
test('every start counts and carries the host CPU idle and load around it', async () => {
  const idles = [90, 80, 70, 60];
  const loads = [1, 2, 3, 4];
  const runsMs = [230, 232];
  const { samples, failures } = await collectSamples({ want: 2, measure: async () => ({ marks: { setupDone: runsMs.shift() }, steps: '', cpuMs: 410 }), idle: () => idles.shift(), load: () => loads.shift() });
  assert.deepEqual(samples.map((s) => [s.idleBefore, s.idleAfter, s.loadBefore, s.loadAfter, s.cpuMs]), [[90, 80, 1, 2, 410], [70, 60, 3, 4, 410]]);
  assert.deepEqual(failures, []);
});

test('a host that is at 0% idle and load 50 still gives every sample', async () => {
  const { samples } = await collectSamples({ want: 5, measure: async () => ({ marks: {}, steps: '', cpuMs: 400 }), idle: () => 0, load: () => 50 });
  assert.equal(samples.length, 5);
});

test('a start that fails is recorded and does not discard the good ones', async () => {
  let n = 0;
  const { samples, failures } = await collectSamples({
    want: 4,
    measure: async () => (++n === 2 ? Promise.reject(new Error('no timings line')) : { marks: {}, steps: '', cpuMs: 400 }),
    idle: () => 50,
  });
  assert.equal(samples.length, 3);
  assert.deepEqual(failures, [{ start: 2, message: 'no timings line' }]);
});

// Wiring: the real script, a fake app and a budgets file made for each case. Exit code, annotations and JSON. The host idle is
// injected (DEBRIEF_BENCH_FAKE_IDLE) and the reference workload cut to 1 reading, so each run takes about a second.
test('script: a busy-host number passes with a note, wall over the ceiling fails, a broken start fails, no timings is not measured', { skip: process.platform === 'win32' }, async () => {
  const { spawnSync } = await import('node:child_process');
  const { readFileSync } = await import('node:fs');
  const { fileURLToPath } = await import('node:url');
  const dir = mkdtempSync(join(tmpdir(), 'debrief-bench-wire-'));
  try {
    const exe = join(dir, 'debrief-app');
    writeFileSync(exe, `#!/bin/sh\ni=0; while [ $i -lt 30000 ]; do i=$((i+1)); done\necho '${line()}'\nexec sleep 30\n`); // burns some CPU so there is a tree CPU time to read
    chmodSync(exe, 0o755);
    const mute = join(dir, 'mute-app');
    writeFileSync(mute, '#!/bin/sh\necho starting\nexit 3\n');
    chmodSync(mute, 0o755);
    const flaky = join(dir, 'flaky-app'); // its third launch (the second counted start) logs nothing
    writeFileSync(flaky, `#!/bin/sh\nc="${dir}/count"; n=$(cat "$c" 2>/dev/null || echo 0); n=$((n+1)); echo $n > "$c"\n[ $n -eq 3 ] && exit 3\necho '${line()}'\nexec sleep 30\n`);
    chmodSync(flaky, 0o755);
    const script = fileURLToPath(new URL('./startup-bench.mjs', import.meta.url));
    let n = 0;
    const go = (budgets, env = {}, app = exe, os = 'macos', ...extra) => {
      const b = join(dir, `budgets-${++n}.json`);
      writeFileSync(b, JSON.stringify({ [os]: budgets }));
      const out = join(dir, `out-${n}.json`);
      const r = spawnSync(process.execPath, [script, app, '--runs', '3', '--os', os, '--budgets', b, '--out', out, '--reference-reps', '1', ...extra], {
        encoding: 'utf8',
        timeout: 60_000,
        env: { ...process.env, GITHUB_REF_TYPE: '', GITHUB_REF: '', DEBRIEF_BENCH_FAKE_IDLE: '90', ...env },
      });
      return { r, json: JSON.parse(readFileSync(out, 'utf8')) };
    };
    const T = (baselineMs, noiseMs) => ({ baselineMs, noiseMs });
    const marks = { firstInteractive: T(100_000, 0) };
    const under = go({ marks, wallCeilingFactor: 3 });
    assert.equal(under.r.status, 0, under.r.stderr);
    assert.equal(under.json.verdict.status, 'pass');
    assert.match(under.r.stdout, /run kind: not a tag/);
    assert.match(under.r.stdout, /host cpu idle 90% -> 90%, load /);
    // firstInteractive 730 ms over a 100 ms noise threshold but under a 100 x ceiling: a note, on a train and on a tag
    for (const env of [{}, { GITHUB_REF_TYPE: 'tag' }]) {
      const busy = go({ marks: { firstInteractive: T(100, 0) }, wallCeilingFactor: 100 }, env);
      assert.equal(busy.r.status, 0, busy.r.stderr);
      assert.match(busy.r.stdout, /note: firstInteractive median 730 ms is over its 100 ms noise threshold/);
      assert.doesNotMatch(busy.r.stderr, /::error::/);
    }
    const ceiling = go({ marks: { firstInteractive: T(100, 0) }, wallCeilingFactor: 3 });
    assert.equal(ceiling.r.status, 1);
    assert.match(ceiling.r.stderr, /::error::start-up benchmark firstInteractive: median 730 ms exceeds the 300 ms ceiling/);
    // A CPU limit on a host with no tree CPU reader (a stand-in reader; --os only picks the budgets row): not applied, said so, the wall time still judged
    const linux = go({ cpu: { ...T(-10, 0), quiet: { ...T(-10, 0), minIdle: 0 } }, marks, wallCeilingFactor: 3 }, { DEBRIEF_BENCH_FAKE_CPU: 'unreadable' }, exe, 'linux');
    assert.equal(linux.r.status, 0, linux.r.stderr);
    assert.match(linux.r.stdout, /note: tree CPU time is not readable on this OS: no CPU limit is applied/);
    if (process.platform === 'darwin') {
      // the tight limit applies at a host idle of minIdle or more, the busy limit otherwise; CPU over the limit fails
      const quiet = go({ cpu: { ...T(1e9, 0), quiet: { ...T(-10, 0), minIdle: 30 } }, marks, wallCeilingFactor: 3 });
      assert.equal(quiet.r.status, 1);
      assert.match(quiet.r.stderr, /::error::start-up benchmark CPU: median .* quiet-host threshold/);
      const busy = go({ cpu: { ...T(1e9, 0), quiet: { ...T(-10, 0), minIdle: 30 } }, marks, wallCeilingFactor: 3 }, { DEBRIEF_BENCH_FAKE_IDLE: '5' });
      assert.equal(busy.r.status, 0, busy.r.stderr);
      assert.match(busy.r.stdout, /note: tree CPU median .* over the quiet-host limit -10 ms, under the busy-host limit/);
      const over = go({ cpu: T(-10, 0), marks, wallCeilingFactor: 3 });
      assert.equal(over.r.status, 1);
      assert.match(over.r.stderr, /::error::start-up benchmark CPU: median .* busy-host threshold/);
    }
    const broken = go({ marks, wallCeilingFactor: 3 }, {}, flaky);
    assert.equal(broken.r.status, 1, 'one start of three gave no timings');
    assert.match(broken.r.stderr, /::error::start-up benchmark: start 2 gave no timings: a broken start/);
    assert.equal(broken.json.runs.length, 2, 'the good starts are kept');
    for (const env of [{}, { GITHUB_REF_TYPE: 'tag' }]) {
      const none = go({ marks, wallCeilingFactor: 3 }, env, mute);
      assert.equal(none.r.status, 6);
      assert.match(none.r.stderr, /::error::start-up benchmark NOT MEASURED.*no start-up timings.*exited \(code 3\)/);
      assert.equal(none.json.verdict.status, 'not-measured');
    }
    const report = go({ marks, wallCeilingFactor: 3 }, {}, mute, 'macos', '--report-only');
    assert.equal(report.r.status, 0);
    const summary = join(dir, 'summary.md');
    go({ marks, wallCeilingFactor: 3 }, { GITHUB_STEP_SUMMARY: summary });
    assert.match(readFileSync(summary, 'utf8'), /### Start-up benchmark \(macos\): pass/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

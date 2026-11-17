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
  const samples = await collectSamples({ want: 2, measure: async () => ({ marks: { setupDone: runsMs.shift() }, steps: '', cpuMs: 410 }), idle: () => idles.shift(), load: () => loads.shift() });
  assert.deepEqual(samples.map((s) => [s.idleBefore, s.idleAfter, s.loadBefore, s.loadAfter, s.cpuMs]), [[90, 80, 1, 2, 410], [70, 60, 3, 4, 410]]);
});

test('a host that is at 0% idle and load 50 still gives every sample', async () => {
  const samples = await collectSamples({ want: 5, measure: async () => ({ marks: {}, steps: '', cpuMs: 400 }), idle: () => 0, load: () => 50 });
  assert.equal(samples.length, 5);
});

test('a start that fails propagates', async () => {
  await assert.rejects(collectSamples({ want: 2, measure: async () => Promise.reject(new Error('no timings line')), idle: () => 50 }), /no timings line/);
});

// Wiring: the real script, a fake app and a budgets file made for each case. Exit code, annotations and JSON.
test('script: a busy-host number passes with a note, CPU over fails, wall over the ceiling fails, no timings is not measured', { skip: process.platform === 'win32' }, async () => {
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
    const script = fileURLToPath(new URL('./startup-bench.mjs', import.meta.url));
    let n = 0;
    const go = (budgets, env = {}, app = exe, ...extra) => {
      const b = join(dir, `budgets-${++n}.json`);
      writeFileSync(b, JSON.stringify({ macos: budgets }));
      const out = join(dir, `out-${n}.json`);
      const r = spawnSync(process.execPath, [script, app, '--runs', '3', '--os', 'macos', '--budgets', b, '--out', out, ...extra], {
        encoding: 'utf8',
        timeout: 60_000,
        env: { ...process.env, GITHUB_REF_TYPE: '', GITHUB_REF: '', ...env },
      });
      return { r, json: JSON.parse(readFileSync(out, 'utf8')) };
    };
    const T = (baselineMs, noiseMs) => ({ baselineMs, noiseMs });
    const under = go({ marks: { firstInteractive: T(100_000, 0) }, wallCeilingFactor: 3 });
    assert.equal(under.r.status, 0, under.r.stderr);
    assert.equal(under.json.verdict.status, 'pass');
    assert.match(under.r.stdout, /run kind: not a tag/);
    assert.match(under.r.stdout, /host cpu idle .* load /);
    assert.match(under.r.stdout, /times it \(a note only\)/);
    // firstInteractive 730 ms over a 100 ms noise threshold but under a 100 x ceiling: a note, on a train and on a tag
    for (const env of [{}, { GITHUB_REF_TYPE: 'tag' }]) {
      const busy = go({ marks: { firstInteractive: T(100, 0) }, wallCeilingFactor: 100 }, env);
      assert.equal(busy.r.status, 0, busy.r.stderr);
      assert.match(busy.r.stdout, /note: firstInteractive median 730 ms is over its 100 ms noise threshold/);
      assert.doesNotMatch(busy.r.stderr, /::error::/);
    }
    const cpu = go({ cpu: T(-10, 0), marks: { firstInteractive: T(100_000, 0) }, wallCeilingFactor: 3 });
    assert.equal(cpu.r.status, 1);
    assert.match(cpu.r.stderr, /::error::start-up benchmark CPU: median/);
    if (process.platform === 'darwin') {
      // the tight limit applies at a host idle of minIdle or more (-1: always), the busy limit otherwise (101: never)
      const marks = { firstInteractive: T(100_000, 0) };
      const quiet = go({ cpu: { ...T(1e9, 0), quiet: { ...T(-10, 0), minIdle: -1 } }, marks, wallCeilingFactor: 3 });
      assert.equal(quiet.r.status, 1);
      assert.match(quiet.r.stderr, /::error::start-up benchmark CPU: median .* quiet-host threshold/);
      const busy = go({ cpu: { ...T(1e9, 0), quiet: { ...T(-10, 0), minIdle: 101 } }, marks, wallCeilingFactor: 3 });
      assert.equal(busy.r.status, 0, busy.r.stderr);
      assert.match(busy.r.stdout, /note: tree CPU median .* over the quiet-host limit -10 ms, under the busy-host limit/);
    }
    const ceiling = go({ marks: { firstInteractive: T(100, 0) }, wallCeilingFactor: 3 });
    assert.equal(ceiling.r.status, 1);
    assert.match(ceiling.r.stderr, /::error::start-up benchmark firstInteractive: median 730 ms exceeds the 300 ms ceiling/);
    for (const env of [{}, { GITHUB_REF_TYPE: 'tag' }]) {
      const none = go({ marks: { firstInteractive: T(100_000, 0) }, wallCeilingFactor: 3 }, env, mute);
      assert.equal(none.r.status, 6);
      assert.match(none.r.stderr, /::error::start-up benchmark NOT MEASURED.*no start-up timings.*exited \(code 3\)/);
      assert.equal(none.json.verdict.status, 'not-measured');
    }
    const report = go({ marks: { firstInteractive: T(100_000, 0) }, wallCeilingFactor: 3 }, {}, mute, '--report-only');
    assert.equal(report.r.status, 0);
    const summary = join(dir, 'summary.md');
    go({ marks: { firstInteractive: T(100_000, 0) }, wallCeilingFactor: 3 }, { GITHUB_STEP_SUMMARY: summary });
    assert.match(readFileSync(summary, 'utf8'), /### Start-up benchmark \(macos\): pass/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

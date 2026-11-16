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

// Collecting samples: no start is dropped for a busy host; each carries the host's CPU idle before and after.
test('every start counts and carries the host CPU idle around it', async () => {
  const idles = [90, 80, 70, 60];
  const runsMs = [230, 232];
  const samples = await collectSamples({ want: 2, measure: async () => ({ marks: { setupDone: runsMs.shift() }, steps: '', cpuMs: 410 }), idle: () => idles.shift() });
  assert.deepEqual(samples.map((s) => [s.idleBefore, s.idleAfter, s.cpuMs]), [[90, 80, 410], [70, 60, 410]]);
});

test('a host that is at 2% idle still gives every sample (the verdict, not a gate, handles it)', async () => {
  const samples = await collectSamples({ want: 5, measure: async () => ({ marks: {}, steps: '', cpuMs: 400 }), idle: () => 2 });
  assert.equal(samples.length, 5);
});

test('a start that fails propagates', async () => {
  await assert.rejects(collectSamples({ want: 2, measure: async () => Promise.reject(new Error('no timings line')), idle: () => 50 }), /no timings line/);
});

// Wiring: the real script, a fake app and a budgets file made for each case. Exit code, annotations and JSON.
test('script: verdicts on a tag and on a train, over and under, from a budgets file', { skip: process.platform === 'win32' }, async () => {
  const { spawnSync } = await import('node:child_process');
  const { readFileSync } = await import('node:fs');
  const { fileURLToPath } = await import('node:url');
  const dir = mkdtempSync(join(tmpdir(), 'debrief-bench-wire-'));
  try {
    const exe = join(dir, 'debrief-app');
    writeFileSync(exe, `#!/bin/sh\ni=0; while [ $i -lt 30000 ]; do i=$((i+1)); done\necho '${line()}'\nexec sleep 30\n`); // burns some CPU so there is a tree CPU time to read
    chmodSync(exe, 0o755);
    const script = fileURLToPath(new URL('./startup-bench.mjs', import.meta.url));
    let n = 0;
    const go = (budgets, env = {}, ...extra) => {
      const b = join(dir, `budgets-${++n}.json`);
      writeFileSync(b, JSON.stringify({ macos: budgets }));
      const out = join(dir, `out-${n}.json`);
      const r = spawnSync(process.execPath, [script, exe, '--runs', '3', '--os', 'macos', '--budgets', b, '--out', out, ...extra], {
        encoding: 'utf8',
        timeout: 60_000,
        env: { ...process.env, GITHUB_REF_TYPE: '', GITHUB_REF: '', ...env },
      });
      return { r, json: JSON.parse(readFileSync(out, 'utf8')) };
    };
    const T = (baselineMs, noiseMs) => ({ baselineMs, noiseMs });
    const steady = { samples: { maxSpread: 1000 } }; // starts that cannot be too unsteady
    const unsteady = { samples: { maxSpread: -1 } }; // a spread limit nothing can meet
    const under = go({ marks: { firstInteractive: T(100_000, 0) }, ...steady });
    assert.equal(under.r.status, 0, under.r.stderr);
    assert.equal(under.json.verdict.status, 'pass');
    assert.match(under.r.stdout, /run kind: not a tag/);
    assert.match(under.r.stdout, /host cpu idle/);
    assert.match(under.r.stdout, /times the reference/);
    const over = go({ marks: { firstInteractive: T(0, 1) }, ...steady });
    assert.equal(over.r.status, 1);
    assert.match(over.r.stderr, /::error::start-up benchmark firstInteractive: median 730 ms exceeds the 1 ms threshold/);
    const trainUnmeasured = go({ marks: { firstInteractive: T(100_000, 0) }, ...unsteady });
    assert.equal(trainUnmeasured.r.status, 0, 'not measured on a train is a note');
    assert.match(trainUnmeasured.r.stdout, /::warning::start-up benchmark NOT MEASURED/);
    assert.doesNotMatch(trainUnmeasured.r.stderr, /::error::/);
    const tagUnmeasured = go({ marks: { firstInteractive: T(100_000, 0) }, ...unsteady }, { GITHUB_REF_TYPE: 'tag' });
    assert.equal(tagUnmeasured.r.status, 6);
    assert.match(tagUnmeasured.r.stderr, /::error::start-up benchmark NOT MEASURED.*ER-33/);
    assert.equal(tagUnmeasured.json.tag, true);
    const report = go({ marks: { firstInteractive: T(0, 1) }, ...unsteady }, { GITHUB_REF_TYPE: 'tag' }, '--report-only');
    assert.equal(report.r.status, 0);
    const summary = join(dir, 'summary.md');
    go({ marks: { firstInteractive: T(100_000, 0) }, ...steady }, { GITHUB_STEP_SUMMARY: summary });
    assert.match(readFileSync(summary, 'utf8'), /### Start-up benchmark \(macos\): pass/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

#!/usr/bin/env node
// The start-up benchmark (Paddock rule 16): starts the built Debrief several times with a throw-away data folder, reads
// each start's one "Startup timings" log line (startup_timings.rs) from its standard output, quits it, and compares the
// median of every mark against the budgets in startup-budgets.json for this OS. The first start (a first run: settings
// suggested, folders created) is reported apart and not counted; the others are the everyday start. A start that never
// logs the line within the time limit fails; nothing is left running (each child is killed, the folder removed).
//
//   node scripts/ci/startup-bench.mjs path/to/debrief-app [--runs 5] [--os macos|linux|windows] [--out timings.json]
//                                     [--report-only]
//
// On macOS the path may be the .app (its main executable is used). `--report-only` prints the medians without
// failing (an OS whose budgets are not measured yet).
//
// Verdict on a busy host (user rule 2026-11-17; the mechanism is scripts/bench/busy-host.mjs, shared): the benchmark is always
// taken and the host's CPU idle and load are recorded with every start; a busy host never makes it "not measured".
//   - FAIL: the CPU time (user + sys) of the started process tree, read when the "Startup timings" line arrives, over its
//     threshold (baseline + measured noise margin); or a wall mark over its ceiling (5 times its baseline), which only a
//     broken start reaches. Both on every run, tag or not.
//   - NOTE, never a failure: a wall mark over its noise threshold but under the ceiling, a high spread of the starts, the
//     ratio to a fixed reference workload, host CPU idle and load.
//   - NOT MEASURED only when the app produced no timings (it did not start or logged no line): ::error:: and exit 6.
// The tree is the app and its sidecars; the WebKit web-content process is not its child on macOS, so its CPU is not in the
// number. Thresholds and their measurement: startup-budgets.json and docs/performance.md, "Start-up benchmark on a busy host".
// `--report-only` prints the numbers and never fails. Exit codes: 0 ok, 1 over a threshold, 2 usage, 6 not measured.
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { loadavg, tmpdir } from 'node:os';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import { annotations, budgetMs, isTagRun, median, readCpuIdle, readTreeCpuMs, referenceReps, referenceStats, verdict } from '../../../../scripts/bench/busy-host.mjs';

export { median };

/** The marks of the line, in order, and their log fields. */
export const MARKS = [
  ['windowCreated', 'WindowCreatedMs'],
  ['setupDone', 'SetupDoneMs'],
  ['pageLoad', 'PageLoadMs'],
  ['firstPaint', 'FirstPaintMs'],
  ['windowReady', 'WindowReadyMs'],
  ['workspaceLoaded', 'WorkspaceLoadedMs'],
  ['firstInteractive', 'FirstInteractiveMs'],
];

/** The marks of a CLEF log line when it is the "Startup timings" line, else null. Pure. */
export function parseTimingsLine(line) {
  let ev;
  try {
    ev = JSON.parse(line);
  } catch {
    return null;
  }
  const text = String(ev?.['@mt'] ?? ev?.['@m'] ?? '');
  if (!text.startsWith('Startup timings')) return null;
  const marks = {};
  for (const [mark, field] of MARKS) {
    const v = Number(ev[field]);
    if (!Number.isFinite(v)) return null;
    marks[mark] = v;
  }
  return { marks, steps: typeof ev.Steps === 'string' ? ev.Steps : '' };
}

/** Medians per mark over the runs, and the marks over budget (`budgets`: mark -> ms). Pure. */
export function evaluate(runs, budgets) {
  const medians = Object.fromEntries(MARKS.map(([m]) => [m, median(runs.map((r) => r.marks[m]))]));
  const over = Object.entries(budgets ?? {})
    .filter(([m, max]) => Number.isFinite(medians[m]) && medians[m] > max)
    .map(([m, max]) => ({ mark: m, median: medians[m], budget: max }));
  return { medians, over };
}

/**
 * Starts `want` times and returns the samples: each carries the host's CPU idle before and after it. A start that fails
 * propagates. `measure()` runs one start; `idle()` reads the host's CPU idle (percent or null) and `load()` the 1-minute load,
 * both recorded with the sample and never used to drop one.
 */
export async function collectSamples({ want, measure, idle, load = () => null }) {
  const samples = [];
  for (let i = 0; i < want; i++) {
    const idleBefore = idle();
    const loadBefore = load();
    const run = await measure();
    samples.push({ ...run, idleBefore, idleAfter: idle(), loadBefore, loadAfter: load() });
  }
  return samples;
}

function executable(path) {
  if (!path.endsWith('.app')) return path;
  const plist = readFileSync(join(path, 'Contents', 'Info.plist'), 'utf8');
  const name = /<key>CFBundleExecutable<\/key>\s*<string>([^<]+)<\/string>/.exec(plist)?.[1];
  if (!name) throw new Error(`no CFBundleExecutable in ${path}`);
  return join(path, 'Contents', 'MacOS', name);
}

/** Starts `exe` once with data folder `data` and resolves with its timings; rejects after `limitMs` without the line. */
export function runOnce(exe, data, limitMs) {
  const child = spawn(exe, [], { env: { ...process.env, DEBRIEF_DATA_DIR: data }, stdio: ['ignore', 'pipe', 'pipe'] });
  child.stderr.resume();
  const done = () =>
    new Promise((resolve) => {
      if (child.exitCode !== null || child.signalCode !== null) return resolve();
      child.once('exit', () => resolve());
      child.kill('SIGTERM');
      setTimeout(() => child.kill('SIGKILL'), 5000).unref();
    });
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => finish(new Error(`no "Startup timings" line within ${limitMs / 1000} s`)), limitMs);
    let settled = false;
    const finish = async (err, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      await done();
      err ? reject(err) : resolve(value);
    };
    createInterface({ input: child.stdout }).on('line', (line) => {
      const t = parseTimingsLine(line);
      if (t) void finish(null, { ...t, cpuMs: readTreeCpuMs(child.pid) });
    });
    child.once('exit', (code) => void finish(new Error(`Debrief exited (code ${code}) before logging its start-up timings`)));
  });
}

async function main() {
  const args = process.argv.slice(2);
  const opt = (name, fallback) => {
    const i = args.indexOf(name);
    return i >= 0 ? args[i + 1] : fallback;
  };
  const exePath = args.find((a, i) => !a.startsWith('--') && !['--runs', '--os', '--out', '--budgets'].includes(args[i - 1]));
  if (!exePath || !existsSync(exePath)) {
    console.error('usage: startup-bench.mjs path/to/debrief-app [--runs 5] [--os macos|linux|windows] [--out file] [--budgets file] [--report-only]');
    process.exit(2);
  }
  const runs = Number(opt('--runs', '5'));
  const os = opt('--os', { darwin: 'macos', win32: 'windows' }[process.platform] ?? 'linux');
  const budgetsFile = opt('--budgets', fileURLToPath(new URL('./startup-budgets.json', import.meta.url)));
  const budgets = JSON.parse(readFileSync(budgetsFile, 'utf8'))[os] ?? null;
  const reportOnly = args.includes('--report-only');
  const tag = isTagRun();
  const exe = executable(exePath);
  const data = mkdtempSync(join(tmpdir(), 'debrief-startup-bench-'));
  const fmt = (v) => (typeof v === 'number' ? v.toFixed(1) : String(v));
  const show = (label, r) =>
    console.log(
      `${label}: ${MARKS.map(([m]) => `${m} ${r.marks[m]}`).join(', ')} ms | steps ${r.steps}` +
        ` | tree cpu ${r.cpuMs === null || r.cpuMs === undefined ? 'n/a' : `${fmt(r.cpuMs)} ms`}` +
        (r.idleBefore === undefined ? '' : ` | host cpu idle ${r.idleBefore ?? 'n/a'}% -> ${r.idleAfter ?? 'n/a'}%, load ${r.loadBefore === null || r.loadBefore === undefined ? 'n/a' : `${fmt(r.loadBefore)} -> ${fmt(r.loadAfter)}`}`),
    );
  console.log(`run kind: ${tag ? 'release tag' : 'not a tag'} (judged alike: a busy host is never "not measured")`);
  let first;
  let startError = null;
  let samples = [];
  let repsBefore;
  let repsAfter;
  try {
    referenceReps(1); // warm-up of the engine's JIT, not counted
    repsBefore = referenceReps(5);
    first = await runOnce(exe, data, 60_000);
    show('first run (not counted)', first);
    samples = await collectSamples({ want: runs, measure: () => runOnce(exe, data, 60_000), idle: () => readCpuIdle(), load: () => (process.platform === 'win32' ? null : loadavg()[0]) });
    samples.forEach((r, i) => show(`run ${i + 1}`, r));
  } catch (e) {
    startError = e.message; // the app did not start or logged no timings: NOT MEASURED below, with this cause
    samples = [];
    console.log(`the app gave no timings: ${startError}`);
  } finally {
    repsAfter = referenceReps(5);
    rmSync(data, { recursive: true, force: true });
  }
  const reps = [...repsBefore, ...repsAfter];
  const { medians } = evaluate(samples, undefined);
  const v = verdict({
    cpuMsSamples: samples.map((r) => r.cpuMs ?? null),
    wallSamples: samples.map((r) => r.marks),
    reference: { reps },
    thresholds: { cpu: budgets?.cpu, marks: budgets?.marks, wallCeilingFactor: budgets?.wallCeilingFactor },
    idle: samples,
    reportOnly: reportOnly || !budgets,
  });
  if (startError) v.notes[0] = `the app produced no start-up timings (${startError})`;
  const ref = referenceStats(reps);
  const num = (x) => (Number.isFinite(x) ? fmt(x) : '-');
  const lines = [
    `reference workload: median ${fmt(ref.median)} ms, spread ${(ref.spread * 100).toFixed(1)}% over ${ref.n} readings (before ${fmt(referenceStats(repsBefore).median)} ms, after ${fmt(referenceStats(repsAfter).median)} ms; idle baseline ${budgets?.reference?.baselineMs ?? '-'} ms); start-up is ${num(v.ratio)} times it (a note only)`,
    `starts: tree cpu median ${num(v.cpuMedian)} ms (threshold ${num(budgetMs(budgets?.cpu) ?? NaN)}), spread of the middle half ${num(v.sampleSpread * 100)}%, median of ${samples.length} starts (${os}):`,
    ...MARKS.map(([m]) => `  ${m.padEnd(17)} wall ${String(medians[m]).padStart(6)} ms   noise threshold ${num(budgetMs(budgets?.marks?.[m]) ?? NaN)}   ceiling ${num(budgets?.marks?.[m] && budgets?.wallCeilingFactor ? budgets.marks[m].baselineMs * budgets.wallCeilingFactor : NaN)}`),
    `verdict: ${v.status.toUpperCase()}`,
    ...v.notes.map((n) => `  note: ${n}`),
  ];
  lines.forEach((l) => console.log(l));
  const summary = process.env.GITHUB_STEP_SUMMARY;
  if (summary) {
    try {
      writeFileSync(summary, `### Start-up benchmark (${os}): ${v.status}\n\n\`\`\`\n${lines.join('\n')}\n\`\`\`\n`, { flag: 'a' });
    } catch (e) {
      console.log(`::warning::could not write the step summary: ${e.message}`);
    }
  }
  const out = opt('--out', null);
  if (out) writeFileSync(out, JSON.stringify({ os, tag, firstRun: first, runs: samples, medians, budgets, reference: { reps, ...ref }, verdict: v }, null, 2));
  for (const a of annotations(v, { name: 'start-up benchmark', os })) (a.startsWith('::warning::') ? console.log : console.error)(a);
  if (v.exitCode !== 0) process.exit(v.exitCode);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error(`::error::${e.message}`);
    process.exit(1);
  });
}

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
// Verdict on a busy host (user decision 2026-11-16; the mechanism is scripts/bench/busy-host.mjs, shared). The gate is the
// CPU time (user + sys) of the started process tree, read when the "Startup timings" line arrives; it does not grow when the
// host is busy. A fixed reference workload is timed before and after the starts, and the wall-time marks are scaled by how
// much slower the host ran it than its recorded baseline. Host CPU idle is printed with every start, never used to drop one.
// The tree is the app and its sidecars; the WebKit web-content process is not its child on macOS, so its CPU is not in the
// number, and the scaled wall time is what covers the rendering marks.
// Thresholds (startup-budgets.json, measured: docs/performance.md, "Start-up benchmark on a busy host"): CPU = recorded
// baseline + measured noise margin; wall marks = budgets scaled to the reference; reference spread limit.
//   - Measured and over a threshold: ::error:: and exit 1, on every run.
//   - Measured and under: pass; the numbers go to the job's step summary.
//   - NOT MEASURED means only that the reference workload was unstable (spread over its limit) or missing. On a tag run
//     (GITHUB_REF_TYPE=tag, ER-33) that is an ::error:: and exit 6; on any other run a ::warning:: and exit 0.
// `--report-only` prints the numbers and never fails. Exit codes: 0 ok, 1 over budget or error, 2 usage, 6 not measured (tag).
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
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
 * propagates. `measure()` runs one start; `idle()` reads the host's CPU idle (percent or null). No sample is dropped for
 * the host being busy: the verdict handles that (scaled wall time, tree CPU).
 */
export async function collectSamples({ want, measure, idle }) {
  const samples = [];
  for (let i = 0; i < want; i++) {
    const idleBefore = idle();
    const run = await measure();
    samples.push({ ...run, idleBefore, idleAfter: idle() });
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
        (r.idleBefore === undefined ? '' : ` | host cpu idle ${r.idleBefore ?? 'n/a'}% -> ${r.idleAfter ?? 'n/a'}%`),
    );
  console.log(`run kind: ${tag ? 'release tag (a measured result is required, ER-33)' : 'not a tag (NOT MEASURED is a warning)'}`);
  let first;
  let samples;
  let repsBefore;
  let repsAfter;
  try {
    referenceReps(1); // warm-up of the engine's JIT, not counted
    repsBefore = referenceReps(5);
    first = await runOnce(exe, data, 60_000);
    show('first run (not counted)', first);
    samples = await collectSamples({ want: runs, measure: () => runOnce(exe, data, 60_000), idle: () => readCpuIdle() });
    repsAfter = referenceReps(5);
    samples.forEach((r, i) => show(`run ${i + 1}`, r));
  } finally {
    rmSync(data, { recursive: true, force: true });
  }
  const reps = [...repsBefore, ...repsAfter];
  const { medians } = evaluate(samples, undefined);
  const v = verdict({
    cpuMsSamples: samples.map((r) => r.cpuMs ?? null),
    wallSamples: samples.map((r) => r.marks),
    reference: { reps, ...budgets?.reference },
    thresholds: { cpu: budgets?.cpu, marks: budgets?.marks, samples: budgets?.samples },
    tag,
    reportOnly: reportOnly || !budgets,
  });
  const ref = referenceStats(reps);
  const num = (x) => (Number.isFinite(x) ? fmt(x) : '-');
  const lines = [
    `reference workload: median ${fmt(ref.median)} ms, spread ${(ref.spread * 100).toFixed(1)}% over ${ref.n} readings (before ${fmt(referenceStats(repsBefore).median)} ms, after ${fmt(referenceStats(repsAfter).median)} ms; baseline ${budgets?.reference?.baselineMs ?? '-'} ms); start-up is ${num(v.ratio)} times the reference${budgets?.reference?.normaliseWall ? ` (wall scaled by ${v.scale.toFixed(3)})` : ' (reported, not used to scale the wall time)'}`,
    `starts: tree cpu median ${num(v.cpuMedian)} ms (threshold ${num(budgetMs(budgets?.cpu) ?? NaN)}), spread of the middle half ${num(v.sampleSpread * 100)}% (limit ${budgets?.samples?.maxSpread === undefined ? '-' : budgets.samples.maxSpread * 100}%), median of ${samples.length} starts (${os}):`,
    ...MARKS.map(([m]) => `  ${m.padEnd(17)} wall ${String(medians[m]).padStart(6)} ms   threshold ${num(budgetMs(budgets?.marks?.[m]) ?? NaN)}`),
    `verdict: ${v.status.toUpperCase()}${v.notes.length ? ` (${v.notes.join('; ')})` : ''}`,
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
  for (const a of annotations(v, { name: 'start-up benchmark', tag, os })) (a.startsWith('::warning::') ? console.log : console.error)(a);
  if (v.exitCode !== 0) process.exit(v.exitCode);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error(`::error::${e.message}`);
    process.exit(1);
  });
}

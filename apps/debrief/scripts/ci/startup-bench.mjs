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
// Load precondition (ER-33): the host's 1-minute load average is read before and after every start and printed with it.
// A start counts only if both readings are <= the threshold (`--max-load N` or DEBRIEF_BENCH_MAX_LOAD, default 12).
// A start that is not counted is not wasted silently: the script waits (`--wait-s`, default 30) and tries again, up to
// `--max-attempts` (default 3 x runs) and `--cap-s` seconds in all (default 900). With fewer than `--runs` valid starts the
// result is NOT MEASURED: one ::error:: names the load seen, and the exit code is 6 (never 0, never 1, which stays
// "over budget"). Windows has no load average: the readings are null and the precondition cannot be checked (said so).
// Exit codes: 0 ok, 1 over budget or error, 2 usage, 6 not measured.
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { loadavg, tmpdir } from 'node:os';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';

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

export function median(values) {
  const v = [...values].sort((a, b) => a - b);
  if (!v.length) return NaN;
  const mid = Math.floor(v.length / 2);
  return v.length % 2 ? v[mid] : (v[mid - 1] + v[mid]) / 2;
}

/** Medians per mark over the runs, and the marks over budget (`budgets`: mark -> ms). Pure. */
export function evaluate(runs, budgets) {
  const medians = Object.fromEntries(MARKS.map(([m]) => [m, median(runs.map((r) => r.marks[m]))]));
  const over = Object.entries(budgets ?? {})
    .filter(([m, max]) => Number.isFinite(medians[m]) && medians[m] > max)
    .map(([m, max]) => ({ mark: m, median: medians[m], budget: max }));
  return { medians, over };
}

export const EXIT_NOT_MEASURED = 6;
export const DEFAULT_MAX_LOAD = 12;

/**
 * The 1-minute load average, or null where the OS has none (Windows reports 0 always). On Windows the comparable signal
 * would be the counter `\System\Processor Queue Length`; not read yet, so a Windows run is "load not verified".
 * Note the 1-minute average lags: a load spike that began seconds ago is not in it yet, so it under-reports a start that
 * just got busy; the check before and after each start narrows that, it does not remove it.
 */
export function readLoad(platform = process.platform, avg = loadavg) {
  return platform === 'win32' ? null : avg()[0];
}

/**
 * Is a sample's load acceptable? Both readings must be <= max. A null (unavailable) reading is "not verified" (ER-33) and
 * is not acceptable, unless `allowUnknown` (a report-only run). An unread `after` (undefined) is skipped. Pure.
 */
export function loadOk(before, after, max, allowUnknown = false) {
  return [before, after].every((l) => (l === undefined ? true : l === null ? allowUnknown : l <= max));
}

/**
 * Collects `want` valid samples: a start counts only if the load before and after is within `max`. `measure()` runs one
 * start; `load()` reads the load; `sleep(ms)` and `now()` are injected. Bounded by `maxAttempts` and `capMs`. Returns
 * { samples, discarded, attempts, peakLoad, measured }. Never throws for load; a failing `measure()` propagates.
 */
export async function collectSamples({ want, max, maxAttempts, waitMs, capMs, measure, load, sleep, now, allowUnknown = false, log = () => {} }) {
  const samples = [];
  const discarded = [];
  let peakLoad = null;
  let unverified = false;
  const seen = (l) => {
    if (typeof l === 'number') peakLoad = peakLoad === null ? l : Math.max(peakLoad, l);
  };
  const t0 = now();
  let attempts = 0;
  const left = () => capMs - (now() - t0);
  while (samples.length < want && attempts < maxAttempts && left() > 0) {
    attempts++;
    const before = load();
    seen(before);
    if (before === null && !allowUnknown) {
      unverified = true;
      log('host load is unavailable: the load precondition cannot be verified');
      break;
    }
    if (!loadOk(before, undefined, max, allowUnknown)) {
      discarded.push({ attempt: attempts, before, after: null, started: false });
      log(`attempt ${attempts}: load ${before} before the start is over ${max}, waiting ${waitMs / 1000} s`);
      if (left() <= waitMs) break;
      await sleep(waitMs);
      continue;
    }
    if (left() <= 0) break;
    const run = await measure();
    const after = load();
    seen(after);
    if (!loadOk(before, after, max, allowUnknown)) {
      discarded.push({ attempt: attempts, before, after, started: true });
      log(`attempt ${attempts}: load ${before} -> ${after} over ${max}, start not counted`);
      if (left() <= waitMs) break;
      await sleep(waitMs);
      continue;
    }
    samples.push({ ...run, loadBefore: before, loadAfter: after });
  }
  return { samples, discarded, attempts, peakLoad, unverified, measured: samples.length >= want };
}

/** The one ::error:: line for a NOT MEASURED result. Pure. */
export function notMeasuredMessage({ samples, discarded, attempts, peakLoad, unverified }, want, max) {
  const load = peakLoad === null ? 'unavailable' : peakLoad.toFixed(1);
  if (unverified) {
    return `::error::start-up benchmark NOT MEASURED: the host load is unavailable on this OS, so the load precondition (<= ${max}) was not verified. Not a pass. Next step: run it on macOS or Linux, or pass --report-only to only print the numbers.`;
  }
  return (
    `::error::start-up benchmark NOT MEASURED: ${samples.length} of ${want} starts ran with host load <= ${max} ` +
    `(${discarded.length} discarded in ${attempts} attempts, peak 1-minute load ${load}). Not a pass. ` +
    `Next step: wait for the host to quiet down (other CI jobs, backups) and re-run the step, or raise DEBRIEF_BENCH_MAX_LOAD only with a measured reason.`
  );
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
      if (t) void finish(null, t);
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
  const exePath = args.find((a, i) => !a.startsWith('--') && !['--runs', '--os', '--out', '--max-load', '--wait-s', '--max-attempts', '--cap-s'].includes(args[i - 1]));
  if (!exePath || !existsSync(exePath)) {
    console.error('usage: startup-bench.mjs path/to/debrief-app [--runs 5] [--os macos|linux|windows] [--out file] [--report-only] [--max-load 12]');
    process.exit(2);
  }
  const runs = Number(opt('--runs', '5'));
  const os = opt('--os', { darwin: 'macos', win32: 'windows' }[process.platform] ?? 'linux');
  const budgetsFile = fileURLToPath(new URL('./startup-budgets.json', import.meta.url));
  const budgets = JSON.parse(readFileSync(budgetsFile, 'utf8'))[os] ?? null;
  const exe = executable(exePath);
  const results = [];
  const data = mkdtempSync(join(tmpdir(), 'debrief-startup-bench-'));
  const flagLoad = opt('--max-load', undefined);
  const envLoad = process.env.DEBRIEF_BENCH_MAX_LOAD;
  const maxLoad = Number(flagLoad ?? envLoad ?? DEFAULT_MAX_LOAD);
  if (!Number.isFinite(maxLoad)) {
    console.error(`usage: --max-load / DEBRIEF_BENCH_MAX_LOAD must be a number, got "${flagLoad ?? envLoad}"`);
    process.exit(2);
  }
  console.log(`load precondition: 1-min load <= ${maxLoad} (source: ${flagLoad !== undefined ? 'flag' : envLoad !== undefined ? 'env' : 'default'})`);
  const waitMs = Number(opt('--wait-s', '30')) * 1000;
  const maxAttempts = Number(opt('--max-attempts', String(runs * 3)));
  const capMs = Number(opt('--cap-s', '900')) * 1000;
  const show = (label, r) =>
    console.log(`${label}: ${MARKS.map(([m]) => `${m} ${r.marks[m]}`).join(', ')} ms | steps ${r.steps}` + (r.loadBefore === undefined ? '' : ` | load ${r.loadBefore} -> ${r.loadAfter}`));
  if (readLoad() === null) console.log('host load is unavailable on this OS: the load precondition cannot be checked');
  let first;
  let collected;
  try {
    first = await runOnce(exe, data, 60_000);
    show('first run (not counted)', first);
    collected = await collectSamples({
      want: runs,
      max: maxLoad,
      maxAttempts,
      waitMs,
      capMs,
      allowUnknown: args.includes('--report-only'),
      measure: () => runOnce(exe, data, 60_000),
      load: () => readLoad(),
      sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
      now: () => Date.now(),
      log: (m) => console.log(m),
    });
    collected.samples.forEach((r, i) => show(`run ${i + 1}`, r));
  } finally {
    rmSync(data, { recursive: true, force: true });
  }
  results.push(...collected.samples);
  const { medians, over } = evaluate(results, budgets?.marks);
  if (collected.measured) {
    console.log(`median of ${results.length} starts (${os}):`);
    for (const [m] of MARKS) console.log(`  ${m.padEnd(17)} ${String(medians[m]).padStart(6)} ms   budget ${budgets?.marks?.[m] ?? '-'}`);
  } else {
    console.log(`no medians: ${results.length} of ${runs} starts met the load precondition`);
  }
  const out = opt('--out', null);
  if (out) writeFileSync(out, JSON.stringify({ os, firstRun: first, runs: results, medians, budgets, load: { max: maxLoad, peak: collected.peakLoad, discarded: collected.discarded, attempts: collected.attempts, measured: collected.measured } }, null, 2));
  if (!collected.measured) {
    const report = args.includes('--report-only');
    const msg = notMeasuredMessage(collected, runs, maxLoad);
    console.error(report ? msg.replace('::error::', '::warning::') : msg);
    if (!report) process.exit(EXIT_NOT_MEASURED);
    return;
  }
  if (!budgets || args.includes('--report-only')) return;
  if (over.length) {
    for (const o of over) console.error(`::error::start-up ${o.mark}: median ${o.median} ms exceeds the ${o.budget} ms budget (${os})`);
    process.exit(1);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error(`::error::${e.message}`);
    process.exit(1);
  });
}

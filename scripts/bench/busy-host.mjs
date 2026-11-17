// The shared mechanism for a timing benchmark that has to give a verdict on a busy host (ER-22: one implementation;
// ER-24: the load average misleads, CPU idle and the work done are what count; ER-33: a measured result on a tag).
// Used by apps/debrief/scripts/ci/startup-bench.mjs. Pure functions take their readings as arguments so tests inject them;
// the readers at the end touch the OS. The rule (user, 2026-11-17): always take the benchmark and record the host load and
// CPU idle with it; never refuse to measure because the host is busy; fail only on a real regression.
// (apps/debrief/docs/performance.md, "Start-up benchmark on a busy host", has the measurements.)
//   1. FAIL when the tree CPU time (user + sys of the started process tree, read when the start is done) is over its
//      limit. There are two: the coarse one (`cpu.baselineMs + cpu.noiseMs`) covers a busy host, where CPU time rises
//      (real build load raised it up to 40%); the tight one (`cpu.quiet`: its own baseline + margin) applies only when the
//      median of the host's CPU idle readings around the starts was at least `cpu.quiet.minIdle` percent (user, 2026-11-17:
//      "tighter when quiet"). Between the two limits on a busy host is a note.
//   2. FAIL when a wall mark is over its ceiling, `ceilingFactor` times its baseline: a limit only a broken start reaches,
//      well above anything a busy host produced.
//   3. NOTE (never a failure): a wall mark over `baselineMs + noiseMs` but under the ceiling, a high spread of the starts,
//      the ratio of start-up to a fixed reference workload, host CPU idle and load. The numbers are printed with the note.
//   4. NOT MEASURED only when the app produced no timings (it did not start, or logged no "Startup timings" line): an
//      ::error:: and exit 6 on every run, since a start that gives nothing is not a pass (ER-33). Report-only warns.
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

/** A quantile of numbers by linear interpolation; NaN for none. Pure. */
export function quantile(values, p) {
  const v = [...values].sort((a, b) => a - b);
  if (!v.length) return NaN;
  const i = (v.length - 1) * p;
  const lo = Math.floor(i);
  return v[lo] + (v[Math.ceil(i)] - v[lo]) * (i - lo);
}

/** The median of numbers; NaN for none. Pure. */
export function median(values) {
  return quantile(values, 0.5);
}

/** `(P75 - P25) / median`: how far the middle half of the readings spread; one outlier does not move it. NaN for none. Pure. */
export function iqrSpread(values) {
  return (quantile(values, 0.75) - quantile(values, 0.25)) / median(values);
}

export const EXIT_OK = 0;
export const EXIT_OVER = 1;
export const EXIT_NOT_MEASURED = 6;

/** Is this a release-tag run? `GITHUB_REF_TYPE=tag`, or `GITHUB_REF` starting `refs/tags/` (Gitea Actions sets both). Pure. */
export function isTagRun(env = process.env) {
  return env.GITHUB_REF_TYPE === 'tag' || String(env.GITHUB_REF ?? '').startsWith('refs/tags/');
}

/** The reference reps' median and spread `(max - min) / median`; NaN for no reps. Pure. */
export function referenceStats(reps) {
  const clean = reps.filter((r) => Number.isFinite(r) && r > 0);
  if (!clean.length) return { median: NaN, spread: NaN, n: 0 };
  const m = median(clean);
  return { median: m, spread: (Math.max(...clean) - Math.min(...clean)) / m, n: clean.length };
}

/** A threshold: the recorded baseline plus the measured noise margin (ER-12), or null when either is missing. Pure. */
export function budgetMs(t) {
  return t && Number.isFinite(t.baselineMs) && Number.isFinite(t.noiseMs) ? t.baselineMs + t.noiseMs : null;
}

/** The wall ceiling of a mark: `ceilingFactor` times its recorded baseline, or null. Pure. */
export function ceilingMs(t, ceilingFactor) {
  return t && Number.isFinite(t.baselineMs) && ceilingFactor > 0 ? t.baselineMs * ceilingFactor : null;
}

/**
 * The verdict of a benchmark. Pure.
 *   cpuMsSamples   tree CPU time per sample in ms (null entries: not readable on this OS)
 *   wallSamples    per sample: { mark: ms } wall marks (no sample at all: the app gave no timings)
 *   reference      { reps: [ms] }  the fixed workload's readings, a note only
 *   thresholds     { cpu: {baselineMs, noiseMs, quiet: {baselineMs, noiseMs, minIdle}}, marks: { mark: {baselineMs, noiseMs} }, wallCeilingFactor }
 *   failures       [{ start, message }] starts that gave no timings. With at least one start that did, each is a FAIL (a broken
 *                  start); with none, the result is NOT MEASURED
 *   cpuExpected    true when this OS can read the tree CPU time (`canReadTreeCpu`): then a missing CPU time with a CPU limit
 *                  defined is a broken check (ER-33), not a note
 *   idle           optional [{ idleBefore, idleAfter, loadBefore, loadAfter }] per sample, summarised in the notes
 *   tag, reportOnly (tag is not used by the verdict: a busy host and a tag run are judged alike; it is kept for callers)
 * Returns { status: 'pass' | 'fail' | 'not-measured', exitCode, over, notes, cpuMedian, ratio, sampleSpread, level }; `level`
 * is the annotation to print for a non-pass: 'error' | 'warning' | null.
 */
export function verdict({ cpuMsSamples, wallSamples, reference, thresholds, idle, failures = [], cpuExpected = false, reportOnly = false }) {
  const ref = referenceStats(reference?.reps ?? []);
  const cpuVals = (cpuMsSamples ?? []).filter((v) => Number.isFinite(v));
  const cpuMedian = cpuVals.length ? median(cpuVals) : NaN;
  const markNames = Object.keys(thresholds?.marks ?? {});
  const lastMark = markNames.at(-1);
  const wallOf = (m) => (wallSamples ?? []).map((w) => w[m]).filter(Number.isFinite);
  const wallMedian = Object.fromEntries(markNames.map((m) => [m, median(wallOf(m))]));
  const judged = cpuVals.length >= 3 ? cpuVals : lastMark ? wallOf(lastMark) : [];
  const sampleSpread = judged.length >= 3 ? iqrSpread(judged) : NaN;
  const ratio = lastMark && ref.median > 0 ? wallMedian[lastMark] / ref.median : NaN;
  const base = { cpuMedian, wallMedian, ratio, sampleSpread, reference: ref };

  if (!cpuVals.length && !Object.values(wallMedian).some((v) => Number.isFinite(v))) {
    const why = failures.length ? ` (${failures.map((f) => `start ${f.start}: ${f.message}`).join('; ')})` : '';
    return { ...base, status: 'not-measured', over: [], exitCode: reportOnly ? EXIT_OK : EXIT_NOT_MEASURED, level: reportOnly ? 'warning' : 'error', notes: [`the app produced no start-up timings${why}`] };
  }

  const notes = [];
  const over = [];
  const cpuCoarse = budgetMs(thresholds?.cpu);
  const quietLimit = budgetMs(thresholds?.cpu?.quiet);
  const minIdle = thresholds?.cpu?.quiet?.minIdle;
  const typicalIdle = medianIdleOf(idle);
  const quiet = quietLimit !== null && Number.isFinite(minIdle) && Number.isFinite(typicalIdle) && typicalIdle >= minIdle;
  base.quiet = quiet;
  base.cpuLimit = quiet ? quietLimit : cpuCoarse;
  if (base.cpuLimit !== null && Number.isFinite(cpuMedian) && cpuMedian > base.cpuLimit) over.push({ kind: 'cpu', median: cpuMedian, budget: base.cpuLimit, quiet });
  else if (!quiet && quietLimit !== null && Number.isFinite(cpuMedian) && cpuMedian > quietLimit) {
    notes.push(`tree CPU median ${cpuMedian.toFixed(0)} ms is over the quiet-host limit ${quietLimit.toFixed(0)} ms, under the busy-host limit ${cpuCoarse.toFixed(0)} ms: a note, because the host was not quiet (median CPU idle ${Number.isFinite(typicalIdle) ? `${typicalIdle.toFixed(0)}%` : 'not read'}, a quiet host needs ${minIdle}% or more)`);
  }
  let broken = false;
  if (cpuCoarse !== null && !Number.isFinite(cpuMedian)) {
    if (cpuExpected) {
      broken = true;
      over.push({ kind: 'broken-cpu' });
    } else notes.push('tree CPU time is not readable on this OS: no CPU limit is applied, only the wall time was judged');
  }
  for (const f of failures) over.push({ kind: 'broken-start', start: f.start, message: f.message });
  for (const m of markNames) {
    const ceiling = ceilingMs(thresholds.marks[m], thresholds.wallCeilingFactor);
    const soft = budgetMs(thresholds.marks[m]);
    const med = wallMedian[m];
    if (!Number.isFinite(med)) continue;
    if (ceiling !== null && med > ceiling) over.push({ kind: 'wall', mark: m, median: med, budget: ceiling });
    else if (soft !== null && med > soft) notes.push(`${m} median ${med.toFixed(0)} ms is over its ${soft.toFixed(0)} ms noise threshold, under the ${ceiling === null ? '-' : ceiling.toFixed(0)} ms ceiling: a note, not a failure (wall time moves with host load)`);
  }
  if (Number.isFinite(sampleSpread)) notes.push(`the starts' own spread (middle half over median) is ${(sampleSpread * 100).toFixed(0)}%`);
  else notes.push('fewer than 3 starts: no spread');
  if (Number.isFinite(ratio)) notes.push(`start-up is ${ratio.toFixed(1)} times the reference workload (median ${ref.median.toFixed(0)} ms, spread ${(ref.spread * 100).toFixed(0)}%)`);
  const hostNote = hostSummary(idle);
  if (hostNote) notes.push(hostNote);
  if (cpuCoarse === null && !markNames.length) notes.push('no thresholds are recorded for this OS: numbers only');
  if (over.length && !reportOnly) return { ...base, status: 'fail', over, exitCode: broken && over.length === 1 ? EXIT_NOT_MEASURED : EXIT_OVER, level: 'error', notes };
  return { ...base, status: 'pass', over, exitCode: EXIT_OK, level: null, notes };
}

/** The median CPU idle percentage over every reading of every sample (before and after), or NaN when none was read. Pure. */
export function medianIdleOf(idle) {
  const v = (idle ?? []).flatMap((s) => [s.idleBefore, s.idleAfter]).filter(Number.isFinite);
  return v.length ? median(v) : NaN;
}

/** "host CPU idle min-max%, load min-max" over the samples' readings, or null. Pure. */
export function hostSummary(idle) {
  const range = (k) => {
    const v = (idle ?? []).flatMap((s) => [s[`${k}Before`], s[`${k}After`]]).filter(Number.isFinite);
    return v.length ? [Math.min(...v), Math.max(...v)] : null;
  };
  const i = range('idle');
  const l = range('load');
  if (!i && !l) return null;
  return `host during the starts: ${i ? `CPU idle ${i[0].toFixed(0)} to ${i[1].toFixed(0)}%` : 'CPU idle n/a'}, ${l ? `1-minute load ${l[0].toFixed(1)} to ${l[1].toFixed(1)}` : 'load n/a'}`;
}

/**
 * The annotation lines for a verdict, with the cause and the next step (ER-27). `nextSteps` is the app's own text:
 * `{ notMeasured, regression, brokenCheck }` (each a sentence; neutral defaults). Pure.
 */
export function annotations(v, { name, os, nextSteps = {} }) {
  const next = {
    notMeasured: 'Next step: run the program by hand and read its log for why it gave no timings.',
    regression: 'Next step: compare the steps of the start with the last green run.',
    brokenCheck: 'Next step: the benchmark itself is broken on this OS, fix the reader before trusting any result.',
    ...nextSteps,
  };
  const out = [];
  if (v.status === 'not-measured') {
    out.push(`${v.level === 'error' ? '::error::' : '::warning::'}${name} NOT MEASURED (${os}): ${v.notes[0]}. Not a pass (ER-33). ${next.notMeasured}`);
  }
  for (const o of v.over) {
    if (o.kind === 'cpu') {
      out.push(`::error::${name} CPU: median ${o.median.toFixed(0)} ms of tree CPU exceeds the ${o.budget.toFixed(0)} ms ${o.quiet ? 'quiet-host' : 'busy-host'} threshold (baseline + margin, ${os}). A real regression: ${o.quiet ? 'the host was quiet' : 'even a busy host does not need this much CPU'}. ${next.regression}`);
    } else if (o.kind === 'wall') {
      out.push(`::error::${name} ${o.mark}: median ${o.median.toFixed(0)} ms exceeds the ${o.budget.toFixed(0)} ms ceiling (${os}), which only a broken start reaches. ${next.regression}`);
    } else if (o.kind === 'broken-start') {
      out.push(`::error::${name}: start ${o.start} gave no timings: a broken start (${o.message}) while other starts did. ${next.notMeasured}`);
    } else if (o.kind === 'broken-cpu') {
      out.push(`::error::${name} check broken (${os}): the tree CPU time could not be read although this OS has a CPU limit (ER-33), so the CPU gate did not run. ${next.brokenCheck}`);
    }
  }
  return out;
}

// ---------------------------------------------------------------------------------------------------------------
// Readers (touch the OS; their parsers are pure and tested).

/** A deterministic, single-threaded integer workload of a fixed size. Returns the checksum so it cannot be optimised away. */
export function referenceWork(iterations = REFERENCE_ITERATIONS) {
  let x = 0x9e3779b9 | 0;
  let acc = 0;
  for (let i = 0; i < iterations; i++) {
    x ^= x << 13;
    x ^= x >>> 17;
    x ^= x << 5;
    acc = (acc + Math.imul(x, 0x85ebca6b)) | 0;
  }
  return acc;
}
/** Chosen so one reading takes about 120 ms on an idle Mac mini (120 M iterations took 580 ms in the first probe) (docs/benchmarks.md records the measurement). */
export const REFERENCE_ITERATIONS = 25_000_000;

/** One timed reference run: { wallMs, cpuMs }. */
export function timeReference(iterations = REFERENCE_ITERATIONS) {
  const c0 = process.cpuUsage();
  const t0 = process.hrtime.bigint();
  referenceWork(iterations);
  const wallMs = Number(process.hrtime.bigint() - t0) / 1e6;
  const c = process.cpuUsage(c0);
  return { wallMs, cpuMs: (c.user + c.system) / 1000 };
}

/** `n` reference readings (wall ms). */
export function referenceReps(n = 5, iterations = REFERENCE_ITERATIONS) {
  return Array.from({ length: n }, () => timeReference(iterations).wallMs);
}

/** Parses ps's cputime (`M:SS.ss`, `H:MM:SS.ss`, `D-HH:MM:SS`) into ms; NaN for anything else. Pure. */
export function parseCpuTime(text) {
  const m = /^\s*(?:(\d+)-)?(?:(\d+):)?(\d+):(\d+(?:\.\d+)?)\s*$/.exec(text);
  if (!m) return NaN;
  const [, d = 0, h = 0, min, s] = m;
  return (((Number(d) * 24 + Number(h)) * 60 + Number(min)) * 60 + Number(s)) * 1000;
}

/** Sums the CPU time of `rootPid` and every descendant from `ps -axo pid=,ppid=,cputime=` output; null if the root is absent. Pure. */
export function treeCpuMs(psOutput, rootPid) {
  const rows = new Map();
  for (const line of psOutput.split('\n')) {
    const m = /^\s*(\d+)\s+(\d+)\s+(\S+)\s*$/.exec(line);
    if (m) rows.set(Number(m[1]), { ppid: Number(m[2]), cpu: parseCpuTime(m[3]) });
  }
  if (!rows.has(rootPid)) return null;
  const inTree = new Set([rootPid]);
  for (let grew = true; grew; ) {
    grew = false;
    for (const [pid, r] of rows) if (!inTree.has(pid) && inTree.has(r.ppid)) (inTree.add(pid), (grew = true));
  }
  let sum = 0;
  for (const pid of inTree) {
    const c = rows.get(pid).cpu;
    if (!Number.isFinite(c)) return null;
    sum += c;
  }
  return sum;
}

/** Can this OS's reader give the tree CPU time? macOS only (Linux `ps` cputime has a 1 s resolution). */
export function canReadTreeCpu(platform = process.platform) {
  return platform === 'darwin';
}

/**
 * The tree CPU time of `pid` now, or null where the OS has no reader or it failed. `-S` adds the time of exited children to
 * their parent, so a child that has already finished (debrief-plugins --manifests) still counts.
 */
export function readTreeCpuMs(pid, platform = process.platform, run = spawnSync) {
  if (!canReadTreeCpu(platform)) return null;
  const r = run('ps', ['-S', '-axo', 'pid=,ppid=,cputime='], { encoding: 'utf8' });
  return r.status === 0 ? treeCpuMs(r.stdout, pid) : null;
}

/**
 * The idle percentage of the last row of `iostat -n 0 -c 2 -w 1` (`us sy id 1m 5m 15m`), or null. The second row is the
 * interval; the first is since boot. (`top -l 2` took 10 s on a host at load 70: it lists every process.) Pure.
 */
export function parseIostatIdle(text) {
  const rows = text.split('\n').filter((l) => /^\s*\d+\s+\d+\s+\d+(\s|$)/.test(l));
  const m = /^\s*\d+\s+\d+\s+(\d+)/.exec(rows.at(-1) ?? '');
  return rows.length >= 2 && m ? Number(m[1]) : null;
}

/** Parses two `/proc/stat` first lines into the idle percentage between them, or null. Pure. */
export function procStatIdle(before, after) {
  const nums = (t) => (/^cpu\s+(.*)$/m.exec(t)?.[1] ?? '').split(/\s+/).map(Number);
  const a = nums(before);
  const b = nums(after);
  if (a.length < 5 || b.length < 5 || [...a, ...b].some((n) => !Number.isFinite(n))) return null;
  const idle = (v) => v[3] + (v[4] ?? 0);
  const total = (v) => v.reduce((s, n) => s + n, 0);
  const dt = total(b) - total(a);
  return dt > 0 ? (100 * (idle(b) - idle(a))) / dt : null;
}

/** Host CPU idle percentage over about one second (macOS: iostat; Linux: /proc/stat); null where unavailable. */
export function readCpuIdle(platform = process.platform, run = spawnSync) {
  if (platform === 'darwin') {
    const r = run('iostat', ['-n', '0', '-c', '2', '-w', '1'], { encoding: 'utf8' });
    return r.status === 0 ? parseIostatIdle(r.stdout) : null;
  }
  if (platform === 'linux') {
    try {
      const a = readFileSync('/proc/stat', 'utf8');
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 1000);
      return procStatIdle(a, readFileSync('/proc/stat', 'utf8'));
    } catch {
      return null;
    }
  }
  return null;
}

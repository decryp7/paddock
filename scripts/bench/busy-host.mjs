// The shared mechanism for a timing benchmark that has to give a verdict on a busy host (ER-22: one implementation;
// ER-24: the load average misleads, CPU idle and the work done are what count; ER-33: a measured result on a tag).
// Used by apps/debrief/scripts/ci/startup-bench.mjs. Pure functions take their readings as arguments so tests inject them;
// the readers at the end touch the OS.
//
// How a verdict is reached (apps/debrief/docs/performance.md, "Start-up benchmark on a busy host", has the measurements):
//   1. CPU time (user + sys) of the started process tree is read when the start is done and gated against
//      `baselineMs + noiseMs`. It does not grow when the host is busy.
//   2. Wall time of each mark is gated the same way. A fixed reference workload (deterministic CPU work of a fixed size)
//      is timed before and after the starts and the ratio of start-up to it is reported. It scales the wall time only when
//      a measurement has shown that it tracks the app (`reference.normaliseWall`): on the Mac it does not (a CPU-bound Node
//      loop slows 2 to 5 times under `yes` loops while the app's start does not move), so there it is off.
//   3. CPU idle is read for every sample and printed, never used to drop one.
//   4. "Not measured" means only that the numbers cannot be trusted: fewer than 3 starts, the starts' own CPU spread over
//      the recorded limit (`samples.maxSpread`, extreme load), or, when the wall time is normalised, an unstable reference.
//      A tag run (ER-33) fails then; any other run warns and stays green.
//      Measured and over a threshold always fails. Measured and under passes.
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
/** Fewer starts than this give no spread to judge. */
export const MIN_SAMPLES = 3;

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

/** The factor that takes the host's slowdown off a wall time: `min(1, baselineMs / medianMs)`. Pure. */
export function wallScale(baselineMs, medianMs) {
  if (!(baselineMs > 0) || !(medianMs > 0)) return 1;
  return Math.min(1, baselineMs / medianMs);
}

/** A threshold: the recorded baseline plus the measured noise margin (ER-12), or null when either is missing. Pure. */
export function budgetMs(t) {
  return t && Number.isFinite(t.baselineMs) && Number.isFinite(t.noiseMs) ? t.baselineMs + t.noiseMs : null;
}

/**
 * The verdict of a benchmark. Pure.
 *   cpuMsSamples   tree CPU time per sample in ms (null entries: not readable on this OS)
 *   wallSamples    per sample: { mark: ms } wall marks
 *   reference      { reps: [ms], baselineMs, maxSpread, normaliseWall }  (reps measured before and after the starts)
 *   thresholds     { cpu: {baselineMs, noiseMs}, marks: { mark: {baselineMs, noiseMs} }, samples: { maxSpread } }
 *   tag, reportOnly
 * Returns { status: 'pass' | 'fail' | 'not-measured', exitCode, over, cpuMedian, ratio, scale, scaledWall, sampleSpread,
 * reference, notes, level }; `level` is the annotation to print for a non-pass: 'error' | 'warning' | null.
 */
export function verdict({ cpuMsSamples, wallSamples, reference, thresholds, tag, reportOnly = false }) {
  const ref = referenceStats(reference?.reps ?? []);
  const normalise = reference?.normaliseWall === true;
  const cpuVals = (cpuMsSamples ?? []).filter((v) => Number.isFinite(v));
  const cpuMedian = cpuVals.length ? median(cpuVals) : NaN;
  const markNames = Object.keys(thresholds?.marks ?? {});
  const lastMark = markNames.at(-1);
  const wallOf = (m) => (wallSamples ?? []).map((w) => w[m]).filter(Number.isFinite);
  const scale = normalise ? wallScale(reference?.baselineMs, ref.median) : 1;
  const scaledWall = Object.fromEntries(markNames.map((m) => [m, median(wallOf(m)) * scale]));
  // The spread of the starts themselves: tree CPU where it is readable, else the wall time of the last mark.
  const judged = cpuVals.length >= MIN_SAMPLES ? cpuVals : lastMark ? wallOf(lastMark) : [];
  const sampleSpread = judged.length >= MIN_SAMPLES ? iqrSpread(judged) : NaN;
  const ratio = lastMark && ref.median > 0 ? median(wallOf(lastMark)) / ref.median : NaN;
  const base = { cpuMedian, ratio, scale, scaledWall, sampleSpread, reference: { ...ref, baselineMs: reference?.baselineMs, maxSpread: reference?.maxSpread, normaliseWall: normalise } };
  const cpuMax = budgetMs(thresholds?.cpu);
  const markBudgets = Object.fromEntries(markNames.map((m) => [m, budgetMs(thresholds.marks[m])]));
  if (cpuMax === null && !Object.values(markBudgets).some((b) => b !== null)) {
    return { ...base, status: 'pass', over: [], exitCode: EXIT_OK, level: null, notes: ['no thresholds are recorded for this OS: numbers only'] };
  }

  const maxSpread = thresholds?.samples?.maxSpread;
  let why = null;
  if (judged.length < MIN_SAMPLES) why = `only ${judged.length} usable starts (at least ${MIN_SAMPLES} are needed to judge their spread)`;
  else if (!Number.isFinite(maxSpread)) why = 'no spread limit for the starts is recorded for this OS';
  else if (Number.isNaN(sampleSpread)) why = "the starts' spread cannot be computed (their median is 0)";
  else if (sampleSpread > maxSpread) why = `the starts' own spread ${(sampleSpread * 100).toFixed(0)}% is over the ${(maxSpread * 100).toFixed(0)}% limit (extreme load)`;
  else if (normalise && ref.n < 2) why = 'the reference workload gave fewer than 2 readings';
  else if (normalise && !Number.isFinite(reference?.maxSpread)) why = 'no reference spread limit is recorded for this OS';
  else if (normalise && ref.spread > reference.maxSpread) why = `the reference workload's spread ${(ref.spread * 100).toFixed(0)}% is over the ${(reference.maxSpread * 100).toFixed(0)}% limit`;
  if (why) {
    const strict = tag && !reportOnly;
    return { ...base, status: 'not-measured', over: [], exitCode: strict ? EXIT_NOT_MEASURED : EXIT_OK, level: strict ? 'error' : 'warning', notes: [why] };
  }

  const over = [];
  if (cpuMax !== null && Number.isFinite(cpuMedian) && cpuMedian > cpuMax) over.push({ kind: 'cpu', median: cpuMedian, budget: cpuMax });
  for (const m of markNames) {
    if (markBudgets[m] !== null && Number.isFinite(scaledWall[m]) && scaledWall[m] > markBudgets[m]) over.push({ kind: 'wall', mark: m, median: scaledWall[m], budget: markBudgets[m] });
  }
  const notes = [];
  if (cpuMax !== null && !Number.isFinite(cpuMedian)) notes.push('tree CPU time is not readable on this OS: only the wall time was judged');
  if (over.length && !reportOnly) return { ...base, status: 'fail', over, exitCode: EXIT_OVER, level: 'error', notes };
  return { ...base, status: 'pass', over, exitCode: EXIT_OK, level: null, notes };
}

/** The annotation lines for a verdict, with the cause and the next step (ER-27). Pure. */
export function annotations(v, { name, tag, os }) {
  const out = [];
  if (v.status === 'not-measured') {
    const l = v.level === 'error' ? '::error::' : '::warning::';
    out.push(
      `${l}${name} NOT MEASURED (${os}): ${v.notes[0]}. ` +
        (tag
          ? 'A release tag needs a measured result (ER-33): not a pass. Next step: re-run the step when the host is quieter; if it stays unstable on a quiet host, the benchmark is broken, investigate it.'
          : 'Not a failure on a non-tag run. Next step: none, or re-run when the host is quieter.'),
    );
  }
  for (const o of v.over) {
    out.push(
      o.kind === 'cpu'
        ? `::error::${name} CPU: median ${o.median.toFixed(0)} ms of tree CPU exceeds the ${o.budget.toFixed(0)} ms threshold (baseline + noise margin, ${os}). A real regression: CPU time does not grow on a busy host. Next step: compare the steps of the start with the last green run.`
        : `::error::${name} ${o.mark}: median ${o.median.toFixed(0)} ms exceeds the ${o.budget.toFixed(0)} ms threshold (baseline + noise margin, ${os}). Next step: compare the steps of the start with the last green run.`,
    );
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

/** The tree CPU time of `pid` now (macOS: 10 ms resolution; Linux: `ps` cputime has 1 s resolution, so null there), or null. */
export function readTreeCpuMs(pid, platform = process.platform, run = spawnSync) {
  if (platform !== 'darwin') return null;
  const r = run('ps', ['-axo', 'pid=,ppid=,cputime='], { encoding: 'utf8' });
  return r.status === 0 ? treeCpuMs(r.stdout, pid) : null;
}

/** The idle percentage of a `top` CPU line (`CPU usage: 3.1% user, 4.2% sys, 92.6% idle`), or null. Pure. */
export function parseTopIdle(text) {
  const lines = text.split('\n').filter((l) => /CPU usage/.test(l));
  const m = /([\d.]+)% idle/.exec(lines.at(-1) ?? '');
  return m ? Number(m[1]) : null;
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

/** Host CPU idle percentage over about one second (macOS: top; Linux: /proc/stat); null where unavailable. */
export function readCpuIdle(platform = process.platform, run = spawnSync) {
  if (platform === 'darwin') {
    const r = run('top', ['-l', '2', '-n', '0', '-s', '1'], { encoding: 'utf8' });
    return r.status === 0 ? parseTopIdle(r.stdout) : null;
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

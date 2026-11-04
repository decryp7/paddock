import type {
  BenchEstimateRequest,
  BenchEstimates,
  BenchRunEstimate,
  Volume,
  BenchKind,
  BenchKindResult,
  BenchLiveness,
  BenchMetric,
  BenchPhaseProgress,
  BenchRun,
  BenchStatus,
  BenchTracePoint,
  BenchVerdict,
} from '../../core/api.types';

const TITLES: Record<BenchKind, string> = { cpu: 'Processor', memory: 'Memory', disk: 'Disk', gpu: 'Graphics' };

/** The plain-language name of a benchmark kind. */
export function kindTitle(kind: BenchKind): string {
  return TITLES[kind];
}

/** Words for a verdict, so colour is never the only signal. */
export function verdictWord(v: BenchVerdict): string {
  return v === 'better' ? 'Better' : v === 'worse' ? 'Worse' : 'No real change';
}

/** Why a kind has no numbers, with the engine's reason. */
function capital(t: string): string {
  return t.charAt(0).toUpperCase() + t.slice(1);
}

export function statusNote(s: BenchStatus): string {
  switch (s.state) {
    case 'done':
      return '';
    case 'stopped':
      // The words "Stopped for safety" are the notice's own heading; the reason follows them.
      return capital(s.reason.replace(/^Stopped for safety:?\s*/, ''));
    default:
      return s.reason;
  }
}

/** The best value of each metric over all runs (the longest bar), so bars are comparable between runs. For latency the best is the smallest. */
export function bestValues(runs: BenchRun[]): Map<string, number> {
  const best = new Map<string, number>();
  for (const r of runs) {
    for (const k of r.results) {
      if (k.status.state !== 'done') continue;
      for (const m of k.status.metrics) {
        const cur = best.get(m.id);
        if (cur === undefined || (m.higherIsBetter ? m.value > cur : m.value < cur)) best.set(m.id, m.value);
      }
    }
  }
  return best;
}

/** How long a metric's bar is, 0..1: its value over the best (or the best over its value for latency). */
export function barFraction(m: BenchMetric, best: Map<string, number>): number {
  const b = best.get(m.id);
  if (!b || !Number.isFinite(m.value) || m.value <= 0) return 0;
  const f = m.higherIsBetter ? m.value / b : b / m.value;
  return Math.min(1, Math.max(0, f));
}

/**
 * The bar of one result against the best earlier run: `fill` is the current value, `marker` the earlier best (null when
 * there is none), both on one scale so the gap between them is visible. Latency is inverted (shorter is better).
 */
export function barPair(m: BenchMetric, ref: number | undefined): { fill: number; marker: number | null } {
  if (!Number.isFinite(m.value) || m.value <= 0) return { fill: 0, marker: null };
  const cur = m.higherIsBetter ? m.value : 1 / m.value;
  if (ref === undefined || !Number.isFinite(ref) || ref <= 0) return { fill: 1, marker: null };
  const old = m.higherIsBetter ? ref : 1 / ref;
  const top = Math.max(cur, old);
  return { fill: cur / top, marker: old / top };
}

/** The spread of a metric's repetitions as plain words: "2,380 to 2,440", or '' for a single measurement. */
export function spreadText(m: BenchMetric, format: (v: number) => string): string {
  if (m.samples.length < 2 || !Number.isFinite(m.min) || !Number.isFinite(m.max)) return '';
  return `${format(m.min)} to ${format(m.max)}`;
}

/** What to say about a run measured with an earlier method (Dyno 1.0's); '' for a current run. */
export function methodNote(run: BenchRun): string {
  return run.oldWay ? 'Measured the old way (Dyno 1.0); not compared with newer runs' : '';
}

/** How far a kind is in words: the passes finished (a pass counts when it ends), so it starts at "0 of 5 passes done". */
export function phaseText(p: BenchPhaseProgress): string {
  // A sustained run counts seconds, not passes.
  if (p.phase === 'cpu-sustained' && p.total) return `${clock(p.count)} of ${clock(p.total)}`;
  return `${p.rep} of ${p.of} ${p.of === 1 ? 'pass' : 'passes'} done`;
}

/** What to tell the person when a run is slow or quiet; '' while it is on plan. Nothing ends the run but their Cancel. */
export function livenessNote(l: BenchLiveness): string {
  switch (l) {
    case 'stillWorking':
      return 'Still working. This is taking longer than planned; Cancel stops it.';
    case 'notResponding':
      return 'Not responding. The measuring program has been quiet for 30 seconds. Wait, or Cancel to stop it.';
    case 'running':
      return '';
  }
}

// ------------------------------------------------------------------------------------------------ presets and estimates (WP-B7)

export type BenchPreset = 'quick' | 'full' | 'sustained' | 'custom';

/** The presets, with the words the picker shows. What each runs is decided in {@link presetRequest}. */
export const PRESETS: { id: BenchPreset; title: string; text: string }[] = [
  { id: 'quick', title: 'Quick', text: 'Processor, memory and the system drive with a small test file' },
  { id: 'full', title: 'Full', text: 'Everything, on the drives you choose, with a 1 GB test file' },
  { id: 'sustained', title: 'Sustained processor', text: 'Runs every core flat out for minutes to show throttling' },
  { id: 'custom', title: 'Custom', text: 'Choose the kinds, the drives and the sizes yourself' },
];

/** The kinds a preset runs; `custom` keeps the person's own choice. */
export function presetKinds(p: BenchPreset, own: readonly BenchKind[]): BenchKind[] {
  switch (p) {
    case 'quick':
      return ['cpu', 'memory', 'disk'];
    case 'full':
      return ['cpu', 'memory', 'disk', 'gpu'];
    case 'sustained':
      return ['cpu'];
    case 'custom':
      return [...own];
  }
}

/** What is chosen, as the page holds it. */
export interface PickedPlan {
  preset: BenchPreset;
  kinds: BenchKind[];
  /** Volume ids; empty means the system drive. */
  drives: string[];
  /** The test file in MiB; null leaves the engine's default. */
  diskMib: number | null;
  sustainedMinutes: number | null;
  sustainedWriteGib: number | null;
  allowNetwork: boolean;
}

/** What to ask the engine to estimate for the plan as picked; the engine does the sums. */
export function estimateRequest(plan: PickedPlan): BenchEstimateRequest {
  return {
    kinds: plan.kinds,
    diskMib: plan.diskMib,
    drives: plan.preset === 'quick' ? 1 : plan.drives.length,
    sustainedMinutes: plan.sustainedMinutes,
    sustainedWriteGib: plan.sustainedWriteGib,
  };
}

/** "Takes about 4 minutes. This writes up to 16 GB to each of the 2 drives."; '' while the engine has nothing to say (no drive chosen). */
export function estimateLine(e: BenchRunEstimate | null, hasDisk: boolean, driveWords: string): string {
  if (!e) return '';
  const writes = hasDisk && e.bytesPerDrive > 0 ? ` ${writesText(e.bytesPerDrive, driveWords)}` : '';
  return `Takes ${aboutTime(e.seconds)}.${writes}`;
}

/** "about 4 minutes", "about 30 seconds". */
export function aboutTime(seconds: number): string {
  if (seconds < 90) return `about ${Math.max(10, Math.round(seconds / 10) * 10)} seconds`;
  const m = Math.round(seconds / 60);
  return `about ${m} minute${m === 1 ? '' : 's'}`;
}

/** "This writes up to 16 GB to Macintosh HD": the engine's upper bound rounded up to whole decimal gigabytes. */
export function writesText(bytes: number, driveName: string): string {
  const gb = Math.ceil(bytes / 1_000_000_000);
  return `This writes up to ${gb} GB to ${driveName}.`;
}

// ------------------------------------------------------------------------------------------------ results: charts and details

/** One point of the memory latency sweep. */
export interface SweepPoint {
  bytes: number;
  ns: number;
  label: string;
}

/** The latency sweep from the metrics (`mem-latency-16k`, `mem-latency-1m`), ascending by size. */
export function sweepPoints(metrics: readonly BenchMetric[]): SweepPoint[] {
  const out: SweepPoint[] = [];
  for (const m of metrics) {
    const hit = /^mem-latency-(\d+)([km])$/.exec(m.id);
    if (!hit) continue;
    const bytes = Number(hit[1]) * (hit[2] === 'k' ? 1024 : 1024 * 1024);
    out.push({ bytes, ns: m.value, label: `${hit[1]} ${hit[2] === 'k' ? 'KiB' : 'MiB'}` });
  }
  return out.sort((a, b) => a.bytes - b.bytes);
}

export interface ChartGeometry {
  /** The `points` attribute of the polyline. */
  points: string;
  /** The largest value, for the axis label. */
  max: number;
  /** The x of the last point and its y, for the end marker. */
  end: { x: number; y: number } | null;
}

/** A polyline over `values` in a `w` by `h` box with `pad` around it; `log` puts the values on a log scale (the latency sweep). */
export function chartGeometry(values: readonly number[], w: number, h: number, pad: number, log = false): ChartGeometry {
  const finite = values.map((v) => (Number.isFinite(v) && v > 0 ? v : 0));
  if (finite.length === 0) return { points: '', max: 0, end: null };
  const f = (v: number) => (log ? Math.log10(Math.max(v, 1e-9)) : v);
  const lo = log ? Math.min(...finite.filter((v) => v > 0).map(f), 0) : 0;
  const hi = Math.max(...finite.map(f), lo + 1e-9);
  const step = finite.length > 1 ? (w - 2 * pad) / (finite.length - 1) : 0;
  const pts = finite.map((v, i) => ({ x: pad + i * step, y: h - pad - ((f(v) - lo) / (hi - lo)) * (h - 2 * pad) }));
  const last = pts[pts.length - 1] ?? null;
  return { points: pts.map((p) => `${p.x.toFixed(1)},${p.y.toFixed(1)}`).join(' '), max: Math.max(...finite), end: last };
}

/** "3,300 MHz · 84 °C" for the newest point of a sustained run; '' when nothing beside the throughput is known. */
export function traceReadout(p: BenchTracePoint | undefined): string {
  if (!p) return '';
  const parts: string[] = [];
  if (p.freqMhz) parts.push(`${Math.round(p.freqMhz).toLocaleString('en-US')} MHz`);
  if (p.tempC !== null && p.tempC !== undefined) parts.push(`${Math.round(p.tempC)} °C`);
  return parts.join(' · ');
}

/** Words for how a sustained run ended when it was stopped; '' otherwise. */
export function stopWords(s: BenchStatus): string {
  return s.state === 'stopped' ? s.reason : '';
}

/** The results of one run grouped for display: a disk result per drive keeps its own card. */
export function resultTitle(r: BenchKindResult): string {
  return r.device ? `${kindTitle(r.kind)} · ${r.device}` : kindTitle(r.kind);
}

/** The drives a run measured, for the history column. */
export function devicesOf(run: BenchRun): string {
  const names = run.results.filter((r) => r.device).map((r) => r.device as string);
  return names.length ? [...new Set(names)].join(', ') : 'This computer';
}

/** The methods a run was measured with, one number per kind, for the history column. */
export function methodsOf(run: BenchRun): string {
  if (run.oldWay) return 'Old (1.0)';
  const m = [...new Set(run.results.map((r) => r.method))].sort();
  return m.length === 1 ? `Method ${m[0]}` : `Methods ${m.join(', ')}`;
}

// ------------------------------------------------------------------------------------------------ review fixes (honest bars, reasons)

/** Seconds as m:ss, the one way a duration is written on this page. */
export function clock(secs: number): string {
  const s = Math.max(0, Math.round(secs));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

/** "1 second", "2 seconds". */
export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** The drive a plain "tick the system drive" default points at: the root, or the Windows system drive. A guess that only sets what is
 * ticked first; the engine decides what is the system drive and re-checks every choice. */
export function systemVolume(volumes: readonly Volume[]): Volume | undefined {
  return volumes.find((v) => v.mount === '/' || v.mount === '/System/Volumes/Data') ?? volumes.find((v) => /^c:\\?$/i.test(v.mount));
}

/** Why a drive row cannot be ticked; '' when it can. */
export function driveBlock(v: Volume, allowNetwork: boolean): string {
  if (v.readOnly) return 'Read-only, so Dyno cannot write a test file to it.';
  if (!v.reachable) return 'Not answering.';
  if (typeof v.location === 'object' && 'network' in v.location && !allowNetwork) return 'Network drive: tick "Include network drives" to test it.';
  return '';
}

/** Why Run is disabled; '' when it is not. Said in words beside the button. */
export function runBlock(p: { kinds: readonly BenchKind[]; drives: readonly string[]; needsDrive: boolean; writeAsked: boolean; writeConfirmed: boolean; writeGib: number }): string {
  if (p.kinds.length === 0) return 'Choose what to measure.';
  if (p.kinds.includes('disk') && p.needsDrive && p.drives.length === 0) return 'Choose at least one drive to test.';
  if (p.kinds.includes('disk') && p.writeAsked && !p.writeConfirmed) return `Tick Yes to confirm the ${p.writeGib} GB write.`;
  return '';
}

/** The words of the sustained-write confirmation, from the engine's numbers for the amount and the ticked drives. */
export function writeConsent(est: BenchEstimates | undefined, gib: number, drives: number): string {
  const n = Math.max(1, drives);
  const secs = (est?.sustainedWrite.find((w) => w.gib === gib)?.seconds ?? 0) * n;
  const total = gib * n;
  const where = n === 1 ? 'the chosen drive' : `each of the ${n} chosen drives (${total} GB in all)`;
  return `This writes ${gib} GB more to ${where} to measure its speed after its fast cache is full, and takes ${aboutTime(secs)} more. It adds wear to ${n === 1 ? 'the drive' : 'each drive'}.`;
}

/** The range of a metric's repetitions on a stated scale: 0 to `scale`, where `scale` is the larger of the highest repetition and the best
 * earlier value. Fractions of the track; `null` for a single measurement, which has no range to draw. */
export function rangeBar(m: BenchMetric, best: number | undefined): { lo: number; hi: number; mid: number; scale: number } | null {
  if (m.samples.length < 2 || !Number.isFinite(m.min) || !Number.isFinite(m.max) || m.max <= 0) return null;
  const scale = Math.max(m.max, best !== undefined && Number.isFinite(best) ? best : 0);
  return { lo: m.min / scale, hi: m.max / scale, mid: m.value / scale, scale };
}

/** A metric whose reads may have come from memory (the old method's, or a file system that refused direct I/O). */
export function cachedReads(m: BenchMetric): boolean {
  return /cached reads possible|may be cached/i.test(m.label);
}

/** Marks a metric the way it was measured: one sample (no spread to show). */
export function singleSample(m: BenchMetric): boolean {
  // A sustained run is one long measurement by design; saying "single sample" on each of its numbers would only be noise.
  return m.samples.length < 2 && !/^(cpu|disk)-sustained/.test(m.id);
}

/** The kind of notice a stopped or unavailable result is, for its icon and words. */
export type NoticeKind = 'safety' | 'cancelled' | 'failed' | 'unavailable';

export function noticeKind(s: BenchStatus): NoticeKind | null {
  switch (s.state) {
    case 'stopped':
      return s.reason.startsWith('Stopped for safety') ? 'safety' : 'cancelled';
    case 'failed':
      return 'failed';
    case 'unavailable':
      return 'unavailable';
    default:
      return null;
  }
}

/** What to do next after a notice; '' when there is nothing to add. */
export function nextStep(k: NoticeKind | null): string {
  switch (k) {
    case 'safety':
      return 'Let it cool, then run again.';
    case 'failed':
      return 'Run it again; if it fails again, save the diagnostics bundle from Diagnostics.';
    default:
      return '';
  }
}

/** The unit of a metric as it reads for its value: "1 episode", "2 episodes". */
export function unitWord(m: BenchMetric): string {
  return m.unit === 'episodes' && m.value === 1 ? 'episode' : m.unit;
}

/** Whether the latest run may be set against the one before: neither measured the old way, and the engine found something comparable. */
export function comparable(latest: BenchRun | null | undefined, previous: BenchRun | null | undefined, deltas: number): boolean {
  return !!latest && !!previous && !latest.oldWay && !previous.oldWay && deltas > 0;
}

/** Whether a disk metric's reads may have come from memory: the runner said the cache was not bypassed, or it is a 1.0 number. */
export function cachedReadsIn(details: readonly string[] | undefined, m: BenchMetric): boolean {
  const read = /-read(-|$)/.test(m.id) || /\bread\b/i.test(m.label);
  return (read && !!details?.some((d) => d.startsWith('Direct I/O: no'))) || /may be cached/i.test(m.label);
}

import type { BenchKind, BenchMetric, BenchRun, BenchStatus, BenchVerdict } from '../../core/api.types';

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
export function statusNote(s: BenchStatus): string {
  switch (s.state) {
    case 'done':
      return '';
    case 'unavailable':
      return `Not available: ${s.reason}`;
    case 'stopped':
      return `Stopped: ${s.reason}`;
    case 'failed':
      return `Failed: ${s.reason}`;
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

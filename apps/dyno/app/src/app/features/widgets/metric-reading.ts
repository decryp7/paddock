import type { CounterReading, SystemSample } from '../../core/api.types';
import type { Metric } from '../../core/generated/widgets';
import { formatPercent, formatRate, splitValue } from '../../shared/format';
import { counterTitle, formatCounter, readingNumber, readingReason } from '../counters/counters-model';
import type { Series } from '../../state/live.store';

/**
 * How a live sample reads for a dashboard widget or a menu-bar preview: the number and its unit as shown, and the live history
 * of a metric. This is display formatting only. Tones, thresholds, limits and which kind accepts which metric are the engine's
 * (the dashboard's values are not in the engine yet: see the report of WP-C2); nothing here decides what is "bad".
 */

/** The metrics offered by a metric picker, in display order (the two parameterised ones are not offered). */
export const METRIC_CHOICES: { metric: Metric; label: string }[] = [
  { metric: 'cpuTotal', label: 'CPU' },
  { metric: 'cpuPerCore', label: 'CPU per core' },
  { metric: 'memory', label: 'Memory' },
  { metric: 'memoryPressure', label: 'Memory pressure' },
  { metric: 'swap', label: 'Swap' },
  { metric: 'diskActivity', label: 'Disk activity' },
  { metric: 'diskFree', label: 'Disk free' },
  { metric: 'networkDown', label: 'Network down' },
  { metric: 'networkUp', label: 'Network up' },
  { metric: 'gpuLoad', label: 'GPU' },
  { metric: 'temperature', label: 'Temperature' },
  { metric: 'fan', label: 'Fan' },
  { metric: 'battery', label: 'Battery' },
];

export function metricLabel(m: Metric): string {
  if (typeof m === 'string') return METRIC_CHOICES.find((x) => x.metric === m)?.label ?? m;
  if ('process' in m) return `Process ${m.process}`;
  return m.perfCounter.startsWith('\\') ? counterTitle(m.perfCounter) : `Counter ${m.perfCounter}`;
}

/** One reading of a metric: a number for tones and charts, and how it reads. */
export interface Reading {
  /** The numeric value (percent for percent metrics), or `null` when unavailable. */
  value: number | null;
  /** Number and unit as shown (`['23.9', 'GB']`). */
  text: [string, string];
  /** 0-100 for bars and rings, or `null` when the metric has no natural scale. */
  percent: number | null;
  /** Why it is unavailable. */
  reason: string | null;
}

const na = (reason: string): Reading => ({ value: null, text: ['—', ''], percent: null, reason });

/** Reads `metric` from a live system sample. Pure. */
export function readMetric(metric: Metric, s: SystemSample | null): Reading {
  if (!s) return na('Waiting for the first reading.');
  const pct = (v: number): Reading => ({ value: v, text: splitValue(formatPercent(v)), percent: v, reason: null });
  switch (metric) {
    case 'cpuTotal':
      return pct(s.cpu.total);
    case 'cpuPerCore':
      return pct(s.cpu.perCore.length ? s.cpu.perCore.reduce((a, b) => a + b, 0) / s.cpu.perCore.length : 0);
    case 'memory':
      return pct(s.memory.total > 0 ? (s.memory.used / s.memory.total) * 100 : 0);
    case 'swap':
      return s.memory.swapTotal > 0 ? pct((s.memory.swapUsed / s.memory.swapTotal) * 100) : na('No swap is configured.');
    case 'memoryPressure': {
      if ('unavailable' in s.memory.pressure) return na(s.memory.pressure.unavailable);
      const p = s.memory.pressure.available;
      return { value: p === 'normal' ? 0 : p === 'elevated' ? 60 : 100, text: [p === 'normal' ? 'Normal' : p === 'elevated' ? 'Elevated' : 'Critical', ''], percent: null, reason: null };
    }
    case 'diskActivity': {
      const bps = s.diskIo.readBps + s.diskIo.writeBps;
      return { value: bps, text: splitValue(formatRate(bps)), percent: null, reason: null };
    }
    case 'diskFree': {
      const d = s.disks[0];
      return d && d.total > 0 ? pct((d.free / d.total) * 100) : na('No drive was reported.');
    }
    case 'networkDown':
    case 'networkUp': {
      const bps = s.network.reduce((a, n) => a + (metric === 'networkDown' ? n.rxBps : n.txBps), 0);
      return { value: bps, text: splitValue(formatRate(bps)), percent: null, reason: null };
    }
    case 'gpuLoad':
      return 'available' in s.gpu ? pct(s.gpu.available.utilization) : na(s.gpu.unavailable);
    case 'temperature': {
      if ('unavailable' in s.temperatures) return na(s.temperatures.unavailable);
      const t = s.temperatures.available[0];
      return t ? { value: t.celsius, text: [t.celsius.toFixed(0), '°C'], percent: null, reason: null } : na('No temperature sensor was reported.');
    }
    case 'fan': {
      if ('unavailable' in s.fans) return na(s.fans.unavailable);
      const f = s.fans.available[0];
      return f ? { value: f.rpm, text: [Math.round(f.rpm).toString(), 'rpm'], percent: null, reason: null } : na('No fan was reported.');
    }
    case 'battery':
      return 'available' in s.battery ? pct(s.battery.available.charge) : na(s.battery.unavailable);
    default:
      return na('This metric is not available yet.');
  }
}

/** Reads a performance counter widget from the counters the engine reads for it (see `CountersStore`). Pure. */
export function readCounter(r: CounterReading | undefined): Reading {
  if (!r) return na('Waiting for the first reading.');
  const n = readingNumber(r);
  if (n === null) return na(readingReason(r) ?? 'The counter is not available.');
  return { value: n, text: formatCounter(n, r.unit), percent: r.unit === 'percent' ? Math.min(100, Math.max(0, n)) : null, reason: null };
}

/** The live history of a metric for a graph (`values2` is the second line, e.g. disk write), or `null` when none is kept. */
export function seriesFor(metric: Metric, s: Series): { values: number[]; values2: number[] | null } | null {
  switch (metric) {
    case 'cpuTotal':
      return { values: s.cpu, values2: null };
    case 'memory':
      return { values: s.memory, values2: null };
    case 'diskActivity':
      return { values: s.diskRead, values2: s.diskWrite };
    case 'networkDown':
      return { values: s.netRx, values2: null };
    case 'networkUp':
      return { values: s.netTx, values2: null };
    case 'gpuLoad':
      return { values: s.gpu, values2: null };
    default:
      return null;
  }
}

/** A metric label inside a sentence: lower case, except an acronym (`CPU`) which stays as it is. */
export function sentenceCase(label: string): string {
  return /^[A-Z]{2,}\b/.test(label) ? label : label.toLowerCase();
}

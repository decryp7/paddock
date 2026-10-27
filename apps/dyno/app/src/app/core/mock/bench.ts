import type { BenchComparison, BenchKind, BenchKindResult, BenchMetric, BenchRun, MetricDelta } from '../api.types';

const NOISE_PERCENT = 3;

const m = (id: string, label: string, value: number, unit: string, higherIsBetter = true): BenchMetric => ({ id, label, value, unit, higherIsBetter });

/** Plausible numbers for one kind; `scale` moves them together (1 = baseline) so a "tuned" run differs from a baseline. */
export function mockKindResult(kind: BenchKind, scale: number, gpu: boolean): BenchKindResult {
  switch (kind) {
    case 'cpu':
      return {
        kind,
        millis: 3400,
        status: {
          state: 'done',
          metrics: [m('cpu-single', 'Single core', 612 * scale, 'M rounds/s'), m('cpu-multi', 'All 10 cores', 5320 * scale, 'M rounds/s'), m('cpu-scaling', 'Multi-core scaling', 8.7, 'x')],
        },
      };
    case 'memory':
      return {
        kind,
        millis: 2300,
        status: { state: 'done', metrics: [m('mem-copy', 'Copy bandwidth', 58.4 * scale, 'GB/s'), m('mem-latency', 'Random access latency', 92 / scale, 'ns', false)] },
      };
    case 'disk':
      return {
        kind,
        millis: 4100,
        status: {
          state: 'done',
          metrics: [
            m('disk-seq-write', 'Sequential write (flushed)', 2410 * scale, 'MB/s'),
            m('disk-seq-read', 'Sequential read (may be cached)', 6120 * scale, 'MB/s'),
            m('disk-rand-read', 'Random 4 KiB reads (may be cached)', 148_000 * scale, 'IOPS'),
          ],
        },
      };
    case 'gpu':
      return gpu
        ? { kind, millis: 0, status: { state: 'done', metrics: [m('gpu-compute', 'Compute', 3100 * scale, 'GFLOPS')] } }
        : { kind, millis: 0, status: { state: 'unavailable', reason: 'GPU compute tests need a graphics API that this build does not include, so none is run.' } };
  }
}

export function mockRun(id: string, startedAt: number, label: string, scale: number, extra: Partial<BenchRun> = {}): BenchRun {
  return {
    id,
    startedAt,
    label,
    group: null,
    phase: null,
    load: { cpuPercent: 6, memoryPercent: 52, onBattery: false },
    machine: 'macOS 15 · Apple M4',
    results: (['cpu', 'memory', 'disk', 'gpu'] as BenchKind[]).map((k) => mockKindResult(k, scale, false)),
    ...extra,
  };
}

/** The engine's `compare`, for the mock only. */
export function mockCompare(before: BenchRun, after: BenchRun): BenchComparison {
  const deltas: MetricDelta[] = [];
  for (const r of before.results) {
    if (r.status.state !== 'done') continue;
    const other = after.results.find((x) => x.kind === r.kind);
    if (!other || other.status.state !== 'done') continue;
    for (const b of r.status.metrics) {
      const a = other.status.metrics.find((x) => x.id === b.id);
      if (!a) continue;
      const change = b.value === 0 ? 0 : ((a.value - b.value) / b.value) * 100;
      const improved = b.higherIsBetter ? change : -change;
      deltas.push({
        kind: r.kind,
        id: b.id,
        label: b.label,
        unit: b.unit,
        higherIsBetter: b.higherIsBetter,
        before: b.value,
        after: a.value,
        changePercent: change,
        verdict: Math.abs(improved) < NOISE_PERCENT ? 'same' : improved > 0 ? 'better' : 'worse',
      });
    }
  }
  const warnings: string[] = [];
  if (before.machine !== after.machine) warnings.push('The runs are from different machines; the comparison means little.');
  if (Math.abs(before.load.cpuPercent - after.load.cpuPercent) > 15) {
    warnings.push(`The background CPU load differed (${Math.round(before.load.cpuPercent)}% before, ${Math.round(after.load.cpuPercent)}% after); repeat the runs on a quiet machine.`);
  }
  return { before: before.id, after: after.id, deltas, warnings };
}

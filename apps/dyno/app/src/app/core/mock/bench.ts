import type { BenchComparison, BenchEstimates, BenchKind, BenchKindResult, BenchMetric, BenchRun, BenchTracePoint, MetricDelta } from '../api.types';

const NOISE_PERCENT = 3;

/** A metric with five repetitions around `value` (a spread of about 1 %), as the engine's `BenchMetric::measured` gives. */
const m = (id: string, label: string, value: number, unit: string, higherIsBetter = true): BenchMetric => {
  const samples = [0.99, 1.0, 1.01, 0.995, 1.005].map((f) => value * f);
  return { id, label, value, unit, higherIsBetter, samples, min: value * 0.99, max: value * 1.01, cv: 0.008, unsteady: false };
};

/** Plausible numbers for one kind; `scale` moves them together (1 = baseline) so a "tuned" run differs from a baseline. */
/** What the engine's `Estimates` say, for the picker in the browser. */
export const MOCK_ESTIMATES: BenchEstimates = {
  kinds: [
    { kind: 'cpu', seconds: 49 },
    { kind: 'memory', seconds: 64 },
    { kind: 'disk', seconds: 0 },
    { kind: 'gpu', seconds: 45 },
  ],
  disk: [
    { mib: 256, bytesWritten: 3_892_314_112, seconds: 110 },
    { mib: 1024, bytesWritten: 15_569_256_448, seconds: 120 },
    { mib: 4096, bytesWritten: 62_277_025_792, seconds: 140 },
  ],
  sustainedMinutes: [2, 5, 10, 20],
  noSensorMaxMinutes: 5,
  sustainedWriteMaxGib: 64,
  quickMib: 256,
  sustainedWrite: [
    { gib: 8, seconds: 17 },
    { gib: 16, seconds: 34 },
    { gib: 32, seconds: 68 },
    { gib: 64, seconds: 137 },
  ],
};

const sweepSizes = [16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536, 131072, 262144, 524288];
const sweepNs = [1.1, 1.1, 1.2, 1.2, 1.9, 3.1, 4.5, 5.2, 6, 11, 17, 33, 62, 89, 96, 98];

/** The latency sweep as the engine reports it: one metric per size, 16 KiB to 512 MiB, and the main memory headline. */
export function mockSweep(scale: number): BenchMetric[] {
  const id = (kib: number) => (kib >= 1024 ? `mem-latency-${kib / 1024}m` : `mem-latency-${kib}k`);
  const label = (kib: number) => (kib >= 1024 ? `${kib / 1024} MiB` : `${kib} KiB`);
  return [...sweepSizes.map((kib, i) => m(id(kib), `Latency at ${label(kib)}`, sweepNs[i]! / scale, 'ns', false)), m('mem-latency', 'Main memory latency', 96 / scale, 'ns', false)];
}

/** A sustained run's trace: full speed, then a fall to `kept` of it from `fallAt` seconds on. */
export function mockTrace(secs: number, peak: number, kept: number, fallAt: number): BenchTracePoint[] {
  return Array.from({ length: secs }, (_, i) => {
    const t = i + 1;
    const f = t < fallAt ? 1 : Math.max(kept, 1 - ((t - fallAt) / 60) * (1 - kept));
    return { secs: t, throughput: peak * f * (1 + Math.sin(t) * 0.004), freqMhz: Math.round(4000 * (0.6 + 0.4 * f)), tempC: Math.min(97, 62 + Math.min(t, 90) * 0.38) };
  });
}

/** Plausible numbers for one kind; `scale` moves them together (1 = baseline) so a "tuned" run differs from a baseline. */
export function mockKindResult(kind: BenchKind, scale: number, gpu: boolean, cores = 14): BenchKindResult {
  return tuned(baseKindResult(kind, scale, gpu, cores), scale);
}

/** A tuned run (scale other than 1) moves each number by its own deterministic few percent, so a comparison is not one flat change. */
function tuned(r: BenchKindResult, scale: number): BenchKindResult {
  if (scale === 1 || r.status.state !== 'done') return r;
  const jitter = (id: string): number => {
    let h = 0;
    for (const c of id) h = (h * 31 + c.charCodeAt(0)) % 97;
    return 1 + ((h % 11) - 5) * 0.014;
  };
  return {
    ...r,
    status: {
      state: 'done',
      metrics: r.status.metrics.map((x) => {
        const f = jitter(x.id);
        return { ...x, value: x.value * f, min: x.min * f, max: x.max * f, samples: x.samples.map((v) => v * f) };
      }),
    },
  };
}

function baseKindResult(kind: BenchKind, scale: number, gpu: boolean, cores = 14): BenchKindResult {
  switch (kind) {
    case 'cpu':
      return {
        kind,
        millis: 49_000,
        method: 3,
        details: ['Vector unit used: NEON.'],
        status: {
          state: 'done',
          metrics: [
            m('cpu-int-single', 'Integer · one core', 612 * scale, 'M rounds/s'),
            m('cpu-int-multi', `Integer · all ${cores} cores`, 612 * cores * 0.62 * scale, 'M rounds/s'),
            m('cpu-sort-single', 'Sorting · one core', 71 * scale, 'M elements/s'),
            m('cpu-sort-multi', `Sorting · all ${cores} cores`, 71 * cores * 0.7 * scale, 'M elements/s'),
            m('cpu-float-single', 'Float (f64) · one core', 12.4 * scale, 'GFLOPS'),
            m('cpu-float-multi', `Float (f64) · all ${cores} cores`, 12.4 * cores * 0.66 * scale, 'GFLOPS'),
            m('cpu-scaling', 'Multi-core scaling (float)', Math.round(cores * 6.6) / 10, 'x'),
            m('cpu-simd-single', 'Vector (f32, NEON) · one core', 98 * scale, 'GFLOPS'),
            m('cpu-simd-multi', `Vector (f32, NEON) · all ${cores} cores`, 98 * cores * 0.6 * scale, 'GFLOPS'),
          ],
        },
      };
    case 'memory':
      return {
        kind,
        millis: 64_000,
        method: 3,
        status: {
          state: 'done',
          metrics: [
            m('mem-stream-copy-single', 'Copy (STREAM-style) · one thread', 62 * scale, 'GB/s'),
            m('mem-stream-copy-multi', 'Copy (STREAM-style) · all threads', 118 * scale, 'GB/s'),
            m('mem-stream-triad-single', 'Triad (STREAM-style) · one thread', 66 * scale, 'GB/s'),
            m('mem-stream-triad-multi', 'Triad (STREAM-style) · all threads', 121 * scale, 'GB/s'),
            ...mockSweep(scale),
          ],
        },
      };
    case 'disk':
      return {
        kind,
        millis: 96_000,
        method: 2,
        device: 'Macintosh HD',
        details: ['Drive: Macintosh HD', 'File system: apfs', 'Medium: solid state', 'Free at the start: 412 GB', 'Direct I/O: yes, the file system\'s cache is bypassed.'],
        status: {
          state: 'done',
          metrics: [
            m('disk-system-seq1m-t1-read', 'Sequential 1 MiB, 1 thread · read', 5100 * scale, 'MB/s'),
            m('disk-system-seq1m-t1-write', 'Sequential 1 MiB, 1 thread · write', 3400 * scale, 'MB/s'),
            m('disk-system-seq1m-t8-read', 'Sequential 1 MiB, 8 threads · read', 5400 * scale, 'MB/s'),
            m('disk-system-seq1m-t8-write', 'Sequential 1 MiB, 8 threads · write', 3500 * scale, 'MB/s'),
            m('disk-system-rand4k-t1-read-iops', 'Random 4 KiB, 1 thread · read (operations)', 14_000 * scale, 'IOPS'),
            m('disk-system-rand4k-t1-read-mbps', 'Random 4 KiB, 1 thread · read', 57 * scale, 'MB/s'),
            m('disk-system-rand4k-t32-write-iops', 'Random 4 KiB, 32 threads · write (operations)', 210_000 * scale, 'IOPS'),
            m('disk-system-rand4k-t32-write-mbps', 'Random 4 KiB, 32 threads · write', 860 * scale, 'MB/s'),
          ],
        },
      };
    case 'gpu':
      return gpu
        ? { kind, millis: 45_000, method: 2, details: ['Graphics adapter: Apple M4 Pro (Metal, driver not reported).'], status: { state: 'done', metrics: [m('gpu-compute', 'Compute throughput', 9200 * scale, 'GFLOPS'), m('gpu-copy', 'Memory copy bandwidth', 230 * scale, 'GB/s'), m('gpu-fill', 'Fill rate', 41 * scale, 'Gpixel/s'), m('gpu-geometry', 'Geometry throughput', 1800 * scale, 'Mtri/s')] } }
        : { kind, millis: 0, method: 2, status: { state: 'unavailable', reason: 'No graphics adapter was found, so there is no GPU to measure. A virtual machine without a virtual GPU has none.' } };
  }
}

/** A metric measured once, as Dyno 1.0 did: no repetitions, so no spread. */
const one = (id: string, label: string, value: number, unit: string, higherIsBetter = true): BenchMetric => ({ id, label, value, unit, higherIsBetter, samples: [value], min: value, max: value, cv: null, unsteady: false });

/** What Dyno 1.0 recorded, and only that: one sample each, the disk reads from the memory cache. */
export function mockOldResults(scale: number, cores = 14): BenchKindResult[] {
  return [
    { kind: 'cpu', millis: 2100, method: 1, status: { state: 'done', metrics: [one('cpu-single', 'Single core', 590 * scale, 'M rounds/s'), one('cpu-multi', `All ${cores} cores`, 5100 * scale, 'M rounds/s'), one('cpu-scaling', 'Multi-core scaling', 8.6, 'x')] } },
    { kind: 'memory', millis: 2300, method: 1, status: { state: 'done', metrics: [one('mem-copy', 'Copy bandwidth', 118 * scale, 'GB/s'), one('mem-latency', 'Random access latency', 96 / scale, 'ns', false)] } },
    { kind: 'disk', millis: 4100, method: 1, status: { state: 'done', metrics: [one('disk-seq-write', 'Sequential write (flushed)', 3400 * scale, 'MB/s'), one('disk-seq-read', 'Sequential read (may be cached)', 15_400 * scale, 'MB/s'), one('disk-rand-read', 'Random 4 KiB reads (may be cached)', 410_000 * scale, 'IOPS')] } },
  ];
}

export function mockRun(id: string, startedAt: number, label: string, scale: number, extra: Partial<BenchRun> = {}, cores = 14): BenchRun {
  return {
    id,
    startedAt,
    label,
    group: null,
    phase: null,
    load: { cpuPercent: 6, memoryPercent: 52, onBattery: false },
    machine: 'macOS 15 · Apple M4 Pro',
    results: extra.oldWay ? mockOldResults(scale, cores) : (['cpu', 'memory', 'disk', 'gpu'] as BenchKind[]).map((k) => mockKindResult(k, scale, true, cores)),
    oldWay: false,
    limited: null,
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
        beforeSpread: b.max - b.min,
        afterSpread: a.max - a.min,
        unsteady: b.unsteady || a.unsteady,
        verdict: Math.abs(a.value - b.value) <= (Math.abs(b.value) * NOISE_PERCENT) / 100 || Math.abs(a.value - b.value) <= b.max - b.min + (a.max - a.min) ? 'same' : improved > 0 ? 'better' : 'worse',
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

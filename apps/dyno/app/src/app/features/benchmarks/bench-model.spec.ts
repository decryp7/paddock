import type { BenchEstimates, BenchKind, BenchMetric, BenchRun, Volume } from '../../core/api.types';
import { mockRun } from '../../core/mock/bench';
import {
  aboutTime,
  cachedReads,
  clock,
  driveBlock,
  nextStep,
  noticeKind,
  plural,
  rangeBar,
  runBlock,
  singleSample,
  systemVolume,
  writeConsent,
  barPair,
  barFraction,
  bestValues,
  chartGeometry,
  devicesOf,
  estimateLine,
  estimateRequest,
  comparable,
  cachedReadsIn,
  unitWord,
  kindTitle,
  livenessNote,
  methodNote,
  methodsOf,
  phaseText,
  presetKinds,
  spreadText,
  statusNote,
  pastNote,
  featuredRun,
  sweepPoints,
  traceReadout,
  verdictWord,
  writesText,
  type PickedPlan,
} from './bench-model';

const metric = (id: string, value: number, higherIsBetter = true): BenchMetric => ({ id, label: id, value, unit: 'u', higherIsBetter, samples: [value], min: value, max: value, cv: 0, unsteady: false });

describe('bench model', () => {
  it('says where a phase is, and how a slow or quiet run is shown', () => {
    expect(phaseText({ phase: 'cpu', label: 'Warming up', rep: 0, of: 5, count: 0, total: 5 })).toBe('0 of 5 passes done');
    expect(phaseText({ phase: 'cpu', label: 'Cooling down before pass 3 of 5', rep: 2, of: 5, count: 2, total: 5 })).toBe('2 of 5 passes done');
    expect(phaseText({ phase: 'cpu-sustained', label: 'Sustained', rep: 16, of: 60, count: 80, total: 300 })).toBe('1:20 of 5:00');
    expect(livenessNote('running')).toBe('');
    expect(livenessNote('stillWorking')).toContain('Still working');
    expect(livenessNote('notResponding')).toContain('Not responding');
    expect(livenessNote('notResponding')).toContain('Cancel');
  });

  it('shows the spread of repetitions in words, and nothing for a single measurement', () => {
    const fmt = (v: number): string => String(v);
    expect(spreadText({ ...metric('x', 100), samples: [98, 100, 102], min: 98, max: 102 }, fmt)).toBe('98 to 102');
    expect(spreadText(metric('x', 100), fmt)).toBe('');
    expect(spreadText({ ...metric('x', 100), samples: [1, 2], min: NaN, max: 2 }, fmt)).toBe('');
  });

  it('words a stored failure in the past, with its date, and never as a current error', () => {
    const unavailable = { state: 'unavailable', reason: 'The benchmark runner was missing from the Dyno 1.0 installation.' } as const;
    const text = pastNote(unavailable, 'Mon 08:25');
    expect(text).toBe('This run on Mon 08:25 could not measure: the benchmark runner was missing from the Dyno 1.0 installation.');
    expect(text).not.toMatch(/reinstall/i);
    expect(pastNote({ state: 'done', metrics: [] }, 'Mon 08:25')).toBe('');
    expect(pastNote({ state: 'failed', reason: 'z' }, 'Mon 08:25')).toBe('z');
  });

  it('features the newest run that measured something, else the newest run', () => {
    const empty = mockRun('n', 3, 'n', 1, { results: [{ kind: 'cpu', status: { state: 'unavailable', reason: 'x' }, millis: 0, method: 1, trace: [], details: [], needsFolder: false, device: null }] });
    const good = mockRun('g', 2, 'g', 1);
    expect(featuredRun([empty, good])?.id).toBe('g');
    expect(featuredRun([empty])?.id).toBe('n');
    expect(featuredRun([])).toBeNull();
  });

  it('marks a run measured the old way and no other', () => {
    expect(methodNote({ ...mockRun('a', 1, 'a', 1), oldWay: true })).toContain('old way');
    expect(methodNote(mockRun('a', 1, 'a', 1))).toBe('');
  });

  it('names kinds and verdicts in words', () => {
    expect(kindTitle('cpu')).toBe('Processor');
    expect(verdictWord('better')).toBe('Better');
    expect(verdictWord('worse')).toBe('Worse');
    expect(verdictWord('same')).toBe('No real change');
  });

  it('explains every non-finished status and has no note for a finished one', () => {
    expect(statusNote({ state: 'done', metrics: [] })).toBe('');
    expect(statusNote({ state: 'unavailable', reason: 'x' })).toBe('x');
    expect(statusNote({ state: 'stopped', reason: 'y' })).toBe('Y');
    expect(statusNote({ state: 'failed', reason: 'z' })).toBe('z');
  });

  it('bars are relative to the best value, inverted for latency', () => {
    const runs: BenchRun[] = [mockRun('a', 1, 'a', 1), mockRun('b', 2, 'b', 2)];
    const best = bestValues(runs);
    expect(barFraction(metric('cpu-int-single', best.get('cpu-int-single')!), best)).toBe(1);
    expect(barFraction(metric('cpu-int-single', best.get('cpu-int-single')! / 2), best)).toBeCloseTo(0.5);
    const lat = best.get('mem-latency')!;
    expect(barFraction(metric('mem-latency', lat * 2, false), best)).toBeCloseTo(0.5);
    expect(barFraction(metric('mem-latency', lat, false), best)).toBe(1);
  });

  it('unknown, zero or non-finite values draw no bar', () => {
    const best = new Map([['x', 10]]);
    expect(barFraction(metric('nope', 5), best)).toBe(0);
    expect(barFraction(metric('x', 0), best)).toBe(0);
    expect(barFraction(metric('x', NaN), best)).toBe(0);
    expect(barFraction(metric('x', 50), best)).toBe(1);
  });

  it('a bar and its earlier-best marker share one scale, inverted for latency', () => {
    expect(barPair(metric('x', 120), 100)).toEqual({ fill: 1, marker: 100 / 120 });
    expect(barPair(metric('x', 80), 100)).toEqual({ fill: 0.8, marker: 1 });
    expect(barPair(metric('lat', 50, false), 100)).toEqual({ fill: 1, marker: 0.5 });
    expect(barPair(metric('x', 5), undefined)).toEqual({ fill: 1, marker: null });
    expect(barPair(metric('x', 0), 5)).toEqual({ fill: 0, marker: null });
  });

  const est: BenchEstimates = {
    kinds: [
      { kind: 'cpu', seconds: 49 },
      { kind: 'memory', seconds: 64 },
      { kind: 'disk', seconds: 0 },
      { kind: 'gpu', seconds: 45 },
    ],
    disk: [
      { mib: 256, bytesWritten: 3_892_314_112, seconds: 90 },
      { mib: 1024, bytesWritten: 15_569_256_448, seconds: 100 },
    ],
    sustainedMinutes: [2, 5, 10, 20],
    noSensorMaxMinutes: 5,
    sustainedWriteMaxGib: 64,
    quickMib: 256,
    sustainedWrite: [],
  };
  const plan = (p: Partial<PickedPlan>): PickedPlan => ({ preset: 'quick', kinds: ['cpu'], drives: [], diskMib: null, sustainedMinutes: null, sustainedWriteGib: null, allowNetwork: false, ...p });

  it('asks the engine for the estimate of what is picked and words what comes back', () => {
    expect(estimateRequest(plan({ preset: 'quick', kinds: ['cpu', 'disk'] }))).toEqual({ kinds: ['cpu', 'disk'], diskMib: null, drives: 1, sustainedMinutes: null, sustainedWriteGib: null });
    expect(estimateRequest(plan({ preset: 'full', kinds: ['disk'], drives: ['/a', '/b'], diskMib: 1024, sustainedWriteGib: 8 })).drives).toBe(2);
    expect(estimateLine({ seconds: 300, bytesPerDrive: 15_569_256_448 }, true, 'Macintosh HD')).toBe('Takes about 5 minutes. This writes up to 16 GB to Macintosh HD.');
    expect(estimateLine({ seconds: 49, bytesPerDrive: 0 }, false, 'x')).toBe('Takes about 50 seconds.');
    expect(estimateLine(null, true, 'x')).toBe('');
  });

  it('says it in words', () => {
    expect(aboutTime(40)).toBe('about 40 seconds');
    expect(aboutTime(113)).toBe('about 2 minutes');
    expect(aboutTime(60 * 5)).toBe('about 5 minutes');
    expect(writesText(15_569_256_448, 'Macintosh HD')).toBe('This writes up to 16 GB to Macintosh HD.');
    expect(presetKinds('quick', [])).toEqual(['cpu', 'memory', 'disk']);
    expect(presetKinds('full', [])).toContain('gpu');
    expect(presetKinds('sustained', ['disk'])).toEqual(['cpu']);
    expect(presetKinds('custom', ['gpu'])).toEqual(['gpu']);
  });

  it('reads the latency sweep from the metric ids and orders it by size', () => {
    const pts = sweepPoints([metric('mem-latency-1m', 9), metric('mem-latency-16k', 1), metric('mem-latency', 99), metric('mem-latency-512m', 90), metric('cpu-int-single', 5)]);
    expect(pts.map((p) => p.label)).toEqual(['16 KiB', '1 MiB', '512 MiB']);
    expect(pts.map((p) => p.ns)).toEqual([1, 9, 90]);
  });

  it('draws a polyline inside its box and survives empty and odd data', () => {
    const g = chartGeometry([10, 20, 40], 100, 50, 5);
    expect(g.points.split(' ')).toHaveLength(3);
    expect(g.max).toBe(40);
    expect(g.end).toEqual({ x: 95, y: 5 });
    expect(chartGeometry([], 100, 50, 5)).toEqual({ points: '', max: 0, end: null });
    expect(chartGeometry([Number.NaN, 5], 100, 50, 5).points).not.toContain('NaN');
    expect(chartGeometry([1, 100, 10000], 100, 50, 5, true).points).not.toContain('NaN');
  });

  it('reads out the clock and temperature beside the newest point and says nothing it does not know', () => {
    expect(traceReadout({ secs: 9, throughput: 100, freqMhz: 3300, tempC: 84.2 })).toBe('3,300 MHz · 84 °C');
    expect(traceReadout({ secs: 9, throughput: 100 })).toBe('');
    expect(traceReadout(undefined)).toBe('');
  });

  it('names the drives and methods of a run for the history', () => {
    const r = mockRun('x', 0, 'x', 1);
    expect(devicesOf(r)).toBe('Macintosh HD');
    expect(devicesOf({ ...r, results: r.results.map((k) => ({ ...k, device: null })) })).toBe('This computer');
    expect(methodsOf({ ...r, oldWay: true })).toBe('Old (1.0)');
    expect(methodsOf(r)).toMatch(/^Methods? /);
  });

  it('writes durations one way and counts in the right plural', () => {
    expect(clock(300)).toBe('5:00');
    expect(clock(49)).toBe('0:49');
    expect(plural(1, 'second')).toBe('1 second');
    expect(plural(2, 'second')).toBe('2 seconds');
  });

  it('says why a drive row or Run is disabled, in words', () => {
    const v = (over: Partial<Volume>): Volume => ({ id: '/v', name: 'V', mount: '/v', fs: 'apfs', location: 'internal', capacity: 1, free: 1, rotational: false, readOnly: false, reachable: true, wholeVolume: true, ...over });
    expect(driveBlock(v({}), false)).toBe('');
    expect(driveBlock(v({ readOnly: true }), false)).toContain('Read-only');
    expect(driveBlock(v({ reachable: false }), false)).toContain('Not answering');
    const net = v({ location: { network: { protocol: 'smb', host: 'nas' } } });
    expect(driveBlock(net, false)).toContain('Include network drives');
    expect(driveBlock(net, true)).toBe('');
    expect(systemVolume([v({ mount: '/Volumes/X' }), v({ mount: '/', id: '/' })])?.id).toBe('/');
    const base = { kinds: ['disk'] as BenchKind[], drives: [] as string[], needsDrive: true, writeAsked: false, writeConfirmed: false, writeGib: 8 };
    expect(runBlock(base)).toBe('Choose at least one drive to test.');
    expect(runBlock({ ...base, drives: ['/'], writeAsked: true })).toBe('Tick Yes to confirm the 8 GB write.');
    expect(runBlock({ ...base, drives: ['/'], writeAsked: true, writeConfirmed: true })).toBe('');
    expect(runBlock({ ...base, kinds: [] })).toBe('Choose what to measure.');
    expect(runBlock({ ...base, kinds: ['cpu'] })).toBe('');
  });

  it('recomputes the sustained-write words from the amount and the ticked drives', () => {
    const e = { ...est, sustainedWrite: [{ gib: 8, seconds: 17 }, { gib: 16, seconds: 34 }] };
    expect(writeConsent(e, 8, 1)).toContain('writes 8 GB more to the chosen drive');
    expect(writeConsent(e, 8, 2)).toContain('each of the 2 chosen drives (16 GB in all)');
    expect(writeConsent(e, 8, 2)).toContain('adds wear to each drive');
    expect(writeConsent(e, 8, 1)).toContain('adds wear to the drive');
    expect(writeConsent(e, 16, 2)).toContain('about 70 seconds');
  });

  it('draws a range on a stated scale and nothing for a single sample', () => {
    const m = { ...metric('x', 100), samples: [90, 100, 110], min: 90, max: 110 };
    expect(rangeBar(m, undefined)).toEqual({ lo: 90 / 110, hi: 1, mid: 100 / 110, scale: 110 });
    expect(rangeBar(m, 220)?.scale).toBe(220);
    expect(rangeBar(metric('x', 100), 50)).toBeNull();
    expect(singleSample(metric('x', 1))).toBe(true);
    expect(singleSample(metric('cpu-sustained-peak', 1))).toBe(false);
    expect(cachedReads({ ...metric('x', 1), label: 'Sequential read (may be cached)' })).toBe(true);
    expect(cachedReads({ ...metric('x', 1), label: 'Sequential 1 MiB · read (cached reads possible)' })).toBe(true);
    expect(cachedReads(metric('x', 1))).toBe(false);
  });

  it('names the kind of a notice and what to do next', () => {
    expect(noticeKind({ state: 'stopped', reason: 'Stopped for safety: hot' })).toBe('safety');
    expect(noticeKind({ state: 'stopped', reason: 'Cancelled.' })).toBe('cancelled');
    expect(noticeKind({ state: 'failed', reason: 'x' })).toBe('failed');
    expect(noticeKind({ state: 'done', metrics: [] })).toBeNull();
    expect(nextStep('safety')).toBe('Let it cool, then run again.');
    expect(nextStep('cancelled')).toBe('');
    expect(statusNote({ state: 'stopped', reason: 'Stopped for safety: the processor stayed hot.' })).toBe('The processor stayed hot.');
  });

  it('does not compare a run measured the old way, or two runs that share nothing', () => {
    const r = mockRun('x', 0, 'x', 1);
    expect(comparable(r, r, 3)).toBe(true);
    expect(comparable({ ...r, oldWay: true }, r, 3)).toBe(false);
    expect(comparable(r, { ...r, oldWay: true }, 3)).toBe(false);
    expect(comparable(r, r, 0)).toBe(false);
    expect(comparable(r, null, 3)).toBe(false);
  });

  it('knows a cached read from the runner\'s note or the 1.0 label, and writes 1 episode in the singular', () => {
    const read = { ...metric('disk-photos-seq1m-t1-read', 1), label: 'Sequential 1 MiB, 1 thread · read' };
    const write = { ...metric('disk-photos-seq1m-t1-write', 1), label: 'Sequential 1 MiB, 1 thread · write' };
    const notes = ['Direct I/O: no, this file system refused it; reads may come from memory.'];
    expect(cachedReadsIn(notes, read)).toBe(true);
    expect(cachedReadsIn(notes, write)).toBe(false);
    expect(cachedReadsIn(['Direct I/O: yes, the file system\'s cache is bypassed.'], read)).toBe(false);
    expect(cachedReadsIn(undefined, { ...read, label: 'Sequential read (may be cached)' })).toBe(true);
    expect(unitWord({ ...metric('e', 1), unit: 'episodes' })).toBe('episode');
    expect(unitWord({ ...metric('e', 2), unit: 'episodes' })).toBe('episodes');
  });
});

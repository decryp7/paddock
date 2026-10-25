import type { Download, MemorySample, Overview, PitwallEvent } from './api.types';
import { INITIAL_LIVE, MAX_EVENTS, MAX_TAIL, appendTail, applyUpdate, upsertDownload, type LiveState } from './live-reducer';
import { MockBackend } from './mock-backend';

const sample = (at: number, extra: Partial<MemorySample> = {}): MemorySample => ({
  at,
  ramTotal: 100,
  ramAvailable: 40,
  commitUsed: null,
  commitLimit: null,
  swapUsed: 0,
  pressure: 'normal',
  cpuPercent: 10,
  gpus: [{ name: 'GPU', vramTotal: 50, vramUsed: 20, utilisation: 5, unified: false }],
  pitwallRss: 7,
  foregroundBusy: false,
  ...extra,
});

const event = (id: number): PitwallEvent => ({ id, at: id, kind: 'download', severity: 'info', message: `e${id}`, modelId: null, appId: null, reason: null });
const dl = (id: string, bytesDone = 0): Download => ({ id, kind: 'model', name: id, host: 'h', bytesDone, bytesTotal: 10, speedBps: 0, state: 'downloading', error: null, startedAt: 0 });

describe('live reducer', () => {
  let overview: Overview;
  beforeEach(async () => {
    overview = await new MockBackend({ now: () => 1_000_000 }).overview();
  });
  const withOverview = (): LiveState => ({ ...INITIAL_LIVE, overview });

  it('folds each update kind into the overview', () => {
    let s = withOverview();
    s = applyUpdate(s, { kind: 'memory', sample: sample(5000) });
    expect(s.overview!.memory.at).toBe(5000);
    expect(s.tail.length).toBe(1);
    s = applyUpdate(s, { kind: 'models', models: [] });
    expect(s.overview!.models).toEqual([]);
    s = applyUpdate(s, { kind: 'queue', queue: { running: [], waiting: [], depth: { interactive: 0, background: 0, batch: 0 } } });
    expect(s.overview!.queue.running).toEqual([]);
    s = applyUpdate(s, { kind: 'throughput', throughput: { promptTps: 1, genTps: 2, ttftP50Ms: 3, requestsPerMin: 4, activeRequests: 0 } });
    expect(s.overview!.throughput.genTps).toBe(2);
    s = applyUpdate(s, { kind: 'issues', issues: [] });
    s = applyUpdate(s, { kind: 'engineSetup', setup: { state: 'failed', error: 'x', download: null } });
    expect(s.overview?.engineSetup.state).toBe('failed');
    expect(s.overview!.issues).toEqual([]);
    s = applyUpdate(s, { kind: 'pairing', pairings: [] });
    expect(s.overview!.pairings).toEqual([]);
    s = applyUpdate(s, { kind: 'server', server: { ...overview.server, serving: 'paused' } });
    expect(s.overview!.server.serving).toBe('paused');
    expect(s.updates).toBe(8);
  });

  it('keeps updates that arrive before the overview without inventing one', () => {
    const s = applyUpdate(INITIAL_LIVE, { kind: 'models', models: [] });
    expect(s.overview).toBeNull();
    expect(applyUpdate(INITIAL_LIVE, { kind: 'memory', sample: sample(1) }).tail.length).toBe(1);
  });

  it('prepends events once, bounded', () => {
    let s = INITIAL_LIVE;
    for (let i = 0; i < MAX_EVENTS + 5; i++) s = applyUpdate(s, { kind: 'event', event: event(i) });
    expect(s.events.length).toBe(MAX_EVENTS);
    expect(s.events[0]!.id).toBe(MAX_EVENTS + 4);
    expect(applyUpdate(s, { kind: 'event', event: event(MAX_EVENTS + 4) })).toBe(s);
  });

  it('upserts downloads', () => {
    let list = upsertDownload([], dl('a'));
    list = upsertDownload(list, dl('b'));
    list = upsertDownload(list, dl('a', 5));
    expect(list.map((d) => [d.id, d.bytesDone])).toEqual([
      ['b', 0],
      ['a', 5],
    ]);
    expect(applyUpdate(INITIAL_LIVE, { kind: 'download', download: dl('x') }).downloads.length).toBe(1);
  });

  it('throttles the tail to one point per second and bounds it', () => {
    let tail = appendTail([], sample(1000), null);
    tail = appendTail(tail, sample(1500), null);
    expect(tail.length).toBe(1);
    tail = appendTail(tail, sample(2000), { promptTps: 9, genTps: 8, ttftP50Ms: 7, requestsPerMin: 0, activeRequests: 0 });
    expect(tail.length).toBe(2);
    expect(tail[1]).toMatchObject({ ramUsed: 60, vramUsed: 20, vramTotal: 50, genTps: 8, p50Ms: 7 });
    expect(tail[0]).toMatchObject({ genTps: 0, p50Ms: null });
    const noGpu = appendTail([], sample(1, { gpus: [] }), null);
    expect(noGpu[0]!.vramUsed).toBeNull();
    let big = Array.from({ length: MAX_TAIL }, (_, i) => appendTail([], sample(i * 1000), null)[0]!);
    big = appendTail(big, sample(MAX_TAIL * 1000), null);
    expect(big.length).toBe(MAX_TAIL);
    expect(big[big.length - 1]!.at).toBe(MAX_TAIL * 1000);
  });
});

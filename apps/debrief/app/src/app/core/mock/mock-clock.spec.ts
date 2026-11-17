import { afterEach, describe, expect, it, vi } from 'vitest';
import { resumeSystemMockClock, stopSystemMockClock, systemMockClock } from './mock-clock';
import { MockBackend } from './mock-backend';
import { MockImportSim } from './mock-import';
import { MockAlertMutes } from './mock-live';
import { query, tinyDataset } from '../../../testing/helpers';
import { ManualClock } from '../../../testing/manual-clock';

describe('mock clock', () => {
  afterEach(() => vi.useRealTimers());

  it('the system clock follows the page timers at each call (fake timers included)', async () => {
    vi.useFakeTimers({ now: 5_000 });
    expect(systemMockClock.now()).toBe(5_000);
    const fired: string[] = [];
    systemMockClock.setTimeout(() => fired.push('timeout'), 100);
    const every = systemMockClock.setInterval(() => fired.push('interval'), 40);
    const dropped = systemMockClock.setTimeout(() => fired.push('never'), 50);
    systemMockClock.clearTimeout(dropped);
    const slept = systemMockClock.sleep(200).then(() => fired.push('slept'));
    await vi.advanceTimersByTimeAsync(100);
    systemMockClock.clearInterval(every);
    await vi.advanceTimersByTimeAsync(200);
    await slept;
    expect(fired).toEqual(['interval', 'interval', 'timeout', 'slept']);
    expect(systemMockClock.now()).toBe(5_300);
    expect(systemMockClock.elapsed()).toBeGreaterThan(0);
  });

  it('a slow import runs in the injected clock: nothing happens until time moves, then exactly the files due', async () => {
    const clock = new ManualClock();
    const sim = new MockImportSim(1_000, clock);
    const read: string[] = [];
    const run = sim.run(['a', 'b', 'c'], (f) => read.push(f));
    await clock.advance(999);
    expect(read).toEqual([]);
    await clock.advance(1);
    expect(read).toEqual(['a']);
    expect(sim.progress()).toMatchObject({ running: true, done: 1, elapsedMs: 1_000 });
    sim.cancel();
    await clock.advance(1_000);
    expect(await run).toEqual({ read: ['a', 'b'], skipped: ['c'] });
  });

  it('live tail ticks and alert mutes run on the backend clock', async () => {
    const clock = new ManualClock();
    const b = new MockBackend({ latency: false, dataset: tinyDataset, pluginMarkerKinds: [], liveRate: 20, liveAlertMs: 1_000, liveRotateMs: 0, clock });
    const alerts: string[] = [];
    await b.onLiveAlert((a) => alerts.push(a.kind));
    await b.setLive(true);
    await clock.advance(999);
    expect(alerts).toEqual([]);
    await clock.advance(1);
    expect(alerts).toEqual(['errorSpike']);
    // A mute is timed on the same clock: it holds the kind back (the next error spike, due in 3 s) until it ends.
    await b.muteAlerts({ kind: 'errorSpike' }, clock.now() + 3_500);
    await clock.advance(3_000);
    expect(alerts).toEqual(['errorSpike', 'rule', 'silence']);
    await clock.advance(3_000);
    expect(alerts).toEqual(['errorSpike', 'rule', 'silence', 'rule', 'silence', 'errorSpike']);
    await b.setLive(false);
    await clock.advance(5_000);
    expect(alerts).toHaveLength(6);
    const mutes = new MockAlertMutes(clock.now);
    mutes.mute({ kind: 'silence' }, clock.now() + 10);
    expect(mutes.covers({ kind: 'silence', rule: null })).toBe(true);
    await clock.advance(10);
    expect(mutes.covers({ kind: 'silence', rule: null })).toBe(false);
  });
});

describe('systemMockClock stop and resume', () => {
  afterEach(() => {
    resumeSystemMockClock();
    vi.useRealTimers();
  });

  it('stop clears the timers and intervals and wakes the sleeps; while stopped nothing new starts and sleeps return at once', async () => {
    vi.useFakeTimers();
    let fired = 0;
    systemMockClock.setTimeout(() => void fired++, 100);
    systemMockClock.setInterval(() => void fired++, 50);
    let woke = false;
    void systemMockClock.sleep(1000).then(() => void (woke = true));
    expect(vi.getTimerCount()).toBe(3);
    stopSystemMockClock('the stop test');
    expect(vi.getTimerCount()).toBe(0);
    await Promise.resolve();
    expect(woke).toBe(true);
    // Stopped: a new timer or interval is refused, a sleep does not wait.
    systemMockClock.setTimeout(() => void fired++, 10);
    systemMockClock.setInterval(() => void fired++, 10);
    expect(vi.getTimerCount()).toBe(0);
    await systemMockClock.sleep(5000);
    await vi.advanceTimersByTimeAsync(1000);
    expect(fired).toBe(0);
  });

  it('resume lets it run again', async () => {
    vi.useFakeTimers();
    stopSystemMockClock();
    resumeSystemMockClock();
    let fired = 0;
    systemMockClock.setTimeout(() => void fired++, 10);
    expect(vi.getTimerCount()).toBe(1);
    await vi.advanceTimersByTimeAsync(10);
    expect(fired).toBe(1);
  });

  it('warns once per pause about a timer asked for between tests', () => {
    const stderr = (globalThis as unknown as { process: { stderr: { write: (s: string) => boolean } } }).process.stderr;
    const real = stderr.write;
    const lines: string[] = [];
    stderr.write = (s: string) => (lines.push(s), true);
    try {
      stopSystemMockClock('a finished test');
      systemMockClock.setTimeout(() => undefined, 1);
      systemMockClock.setInterval(() => undefined, 1);
      expect(lines.filter((l) => l.includes('[mock-clock]'))).toHaveLength(1);
      // It names the test the clock was stopped after and the caller (this spec).
      expect(lines.find((l) => l.includes('[mock-clock]'))).toMatch(/after "a finished test".* at .*mock-clock\.spec\.ts/);
      resumeSystemMockClock();
      stopSystemMockClock();
      systemMockClock.setTimeout(() => undefined, 1);
      expect(lines.filter((l) => l.includes('[mock-clock]'))).toHaveLength(2);
    } finally {
      stderr.write = real;
    }
  });
});

describe('page stagger', () => {
  it('answers a later page later, so the pages of one request arrive at different times', async () => {
    const clock = new ManualClock();
    const backend = new MockBackend({ latency: true, dataset: tinyDataset, syntheticTotal: 5000, pageStaggerMs: 10, clock });
    const answered: number[] = [];
    const ask = (page: number) => void backend.queryPage({ query: query(), offset: page * 500, limit: 500, descending: false, anchor: null }).then(() => answered.push(page));
    ask(0);
    ask(4);
    await clock.advance(20);
    expect(answered).toEqual([0]);
    await clock.advance(40);
    expect(answered).toEqual([0, 4]);
  });
});

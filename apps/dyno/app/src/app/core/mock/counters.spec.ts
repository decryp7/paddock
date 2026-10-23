import { MockCounters, counterValue, demoCounterSettings } from './counters';

const NOW = 1_700_000_000_000;

describe('MockCounters on a Mac', () => {
  const m = new MockCounters('macos');

  it('offers the host statistics under Dyno\'s names, and the demo settings use only counters that exist', () => {
    expect(m.objects().objects.map((o) => o.name)).toEqual(['Memory', 'Virtual Memory', 'Network Interface', 'Physical Disk', 'System']);
    for (const os of ['macos', 'windows'] as const) {
      const demo = demoCounterSettings(os);
      const reader = new MockCounters(os);
      const paths = [...demo.pinned, ...demo.sets.flatMap((s) => s.paths)];
      for (const r of reader.values(paths, NOW)) expect(r.value, r.path).toHaveProperty('available');
    }
  });
});

describe('MockCounters', () => {
  const m = new MockCounters('windows');

  it('lists objects with their sizes and opens one', () => {
    expect(m.objects().objects.find((o) => o.name === 'PhysicalDisk')).toEqual({ name: 'PhysicalDisk', counters: 4, instancesTotal: 2 });
    expect(m.object('Memory').instances).toEqual([]);
    expect(() => m.object('Nope')).toThrow();
  });

  it('searches by words, best match first, with one hit per instance', () => {
    const hits = m.search('bytes received', 10);
    expect(hits.map((h) => h.path)).toEqual(['\\Network Interface(Ethernet)\\Bytes Received/sec', '\\Network Interface(Wi-Fi)\\Bytes Received/sec']);
    expect(m.search('', 10)).toEqual([]);
    expect(m.search('e', 3)).toHaveLength(3);
  });

  it('reads known counters in their range and unknown ones as gone', () => {
    const [cpu, gone, badInstance] = m.values(['\\Processor Information(_Total)\\% Processor Time', '\\Nope\\x', '\\PhysicalDisk(9)\\% Idle Time'], NOW);
    expect('available' in cpu!.value && cpu!.value.available >= 0 && cpu!.value.available <= 100).toBe(true);
    expect(gone!.value).toHaveProperty('unavailable');
    expect(badInstance!.value).toHaveProperty('unavailable');
  });

  it('is deterministic and gives a history with events inside its window', () => {
    expect(counterValue('\\A\\b', 'bytes', 5000)).toBe(counterValue('\\A\\b', 'bytes', 5000));
    const h = m.history(['\\Memory\\Pages/sec', '\\Nope\\x'], 5, NOW);
    expect(h.to - h.from).toBe(5 * 60_000);
    expect(h.series[0]!.points.length).toBeGreaterThan(50);
    expect(h.series[0]!.points.every((p) => p.at >= h.from && p.at <= h.to)).toBe(true);
    expect(h.series[1]!.points).toEqual([]);
    expect(h.events.every((e) => e.at >= h.from && e.at <= h.to)).toBe(true);
    expect(m.history([], 500, NOW).to - m.history([], 500, NOW).from).toBe(60 * 60_000);
  });
});

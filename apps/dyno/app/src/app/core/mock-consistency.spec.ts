import { formatRate, formatStorage } from '../shared/format';
import { readValue } from './measured';
import { MockBackend, type MockOptions } from './mock-backend';
import {
  MAC_BUILD_CPU,
  MAC_BUILD_DISK_BPS,
  MAC_CARGO_CPU,
  MAC_CPU_TOTAL,
  MAC_DISK_FREE,
  MAC_DISK_FREE_TEXT,
  MAC_DISK_IO_BPS,
  MAC_DISK_TOTAL,
  MAC_DISK_USED_PERCENT,
  MAC_MEMORY_PERCENT,
  MAC_MEMORY_TOTAL,
  MAC_MEMORY_USED,
} from './mock/mac-machine';

/**
 * One fact reads the same on every page (the class "mock facts drift across pictures": the startup disk was 182 of 994 GB in the tray, 210 of 953 on Health
 * and 225 of 1 TB on the disk map, the build wrote 189 MB/s on Processes and 121 MB/s on Why slow). The mock's Mac has one fact sheet
 * (mock/mac-machine.ts) and its replies and texts read it; this spec asks every reply that names the same fact and says they agree.
 */
const NOW = 1_700_000_000_000;
// `stableMs` as the pictures use it: the polled replies and the live readings read the same minute, whose start is the reference second.
const make = (o: MockOptions = {}) => new MockBackend({ latencyMs: 0, now: () => NOW, os: 'macos', stableMs: 60_000, ...o });

describe('the mock Mac says one thing about its startup disk', () => {
  it('the disk map, the machine inventory, the live readings, the history, the weekly report and the tray agree on its size and free space', async () => {
    const b = make();
    const volume = (await b.diskVolumes()).find((v) => v.mount === '/')!;
    expect([volume.capacity, volume.free]).toEqual([MAC_DISK_TOTAL, MAC_DISK_FREE]);

    const inventory = await b.inventory();
    expect(inventory.disks[0]!.totalBytes).toBe(MAC_DISK_TOTAL);

    const live = b.liveUpdate('summary').system!.disks[0]!;
    expect([live.total, live.free]).toEqual([MAC_DISK_TOTAL, MAC_DISK_FREE]);

    const trend = (await b.history('week', null)).diskTrends[0]!;
    expect(trend.total).toBe(MAC_DISK_TOTAL);
    expect(trend.points.at(-1)!.free).toBe(MAC_DISK_FREE);

    const report = (await b.weeklyReport(1)).disks[0]!;
    expect([report.total, report.free]).toEqual([MAC_DISK_TOTAL, MAC_DISK_FREE]);

    const row = JSON.stringify(await b.popoverValues());
    expect(row).toContain(String(MAC_DISK_FREE));
    expect(row).toContain(String(MAC_DISK_TOTAL));
  });

  it('every text that names it is built from the same numbers, with the app\'s own formatter', async () => {
    const b = make();
    expect(MAC_DISK_FREE_TEXT).toBe(`${formatStorage(MAC_DISK_FREE)} free of ${formatStorage(MAC_DISK_TOTAL)}`);
    const alarm = (await b.alarms({ states: [], minSeverity: null, rule: null, includeShelved: true, limit: 500 })).find((a) => a.rule === 'alert:lowDisk')!;
    expect(alarm.title).toBe(`Macintosh HD is ${MAC_DISK_USED_PERCENT}% full`);
    expect(alarm.detail).toBe(`${MAC_DISK_FREE_TEXT}.`);
    const card = (await b.whySlow()).causes.find((c) => c.kind === 'lowDiskSpace')!;
    expect(card.title).toBe(`Macintosh HD is ${MAC_DISK_USED_PERCENT}% full`);
    expect(card.evidence[0]!.value).toBe(`${formatStorage(MAC_DISK_FREE)} of ${formatStorage(MAC_DISK_TOTAL)}`);
    const issue = (await b.weeklyReport(1)).highlights.length >= 0 ? (await b.weeklyReport(1)) : null;
    expect(JSON.stringify(issue)).toContain(MAC_DISK_FREE_TEXT);
  });
});

describe('the build writes one figure on every page', () => {
  it('Why slow quotes the sum of the rustc processes\' writes that Processes lists', async () => {
    const b = make();
    const written = b.liveUpdate('processes').processes!.filter((p) => p.name === 'rustc').reduce((a, p) => a + (readValue(p.diskWriteBps) ?? 0), 0);
    const disk = (await b.whySlow()).causes.find((c) => c.kind === 'diskQueue')!;
    expect(disk.evidence.find((e) => e.label === 'Writes')!.value).toBe(`${formatRate(written)} by rustc`);
    expect(disk.explanation).toContain(`rustc writes ${formatRate(written)} of build output`);
  });
});

describe('the load, the memory and the disk traffic are the declared figures on every page', () => {
  it('the live readings and the process table sum to the sheet\'s CPU, build, memory and disk traffic', () => {
    const live = make().liveUpdate('processes');
    expect(Math.round(live.system!.cpu.total)).toBe(MAC_CPU_TOTAL);
    const build = live.groups!.find((g) => g.id === 'cargo')!;
    expect(Math.round(readValue(build.cpu) ?? -1)).toBe(MAC_BUILD_CPU);
    expect(readValue(live.processes!.find((p) => p.name === 'cargo')!.cpu)).toBeCloseTo(MAC_CARGO_CPU, 1);
    expect(Math.abs((readValue(build.diskBps) ?? 0) - MAC_BUILD_DISK_BPS)).toBeLessThan(1024);
    const traffic = live.system!.diskIo.readBps + live.system!.diskIo.writeBps;
    expect(Math.abs(traffic - MAC_DISK_IO_BPS)).toBeLessThan(1024);
    expect(Math.abs(live.system!.memory.used - MAC_MEMORY_USED)).toBeLessThan(1024 ** 2);
    expect(live.system!.memory.total).toBe(MAC_MEMORY_TOTAL);
  });

  it('Why slow, Throttling, Is this normal and the tray say the same load and memory', async () => {
    const b = make();
    const cause = (await b.whySlow()).causes.find((c) => c.kind === 'cpuHog')!;
    expect(cause.title).toBe(`rustc is using ${Math.round(MAC_BUILD_CPU - MAC_CARGO_CPU)}% CPU`);
    expect(cause.evidence.find((e) => e.label === 'CPU total')!.value).toBe(`${MAC_CPU_TOTAL}%`);
    const throttling = await b.throttling();
    expect(throttling.reading.evidence.find((e) => e.label === 'Load')!.value).toBe(`${MAC_CPU_TOTAL}%`);
    const items = (await b.baselines()).items;
    expect(items.find((i) => i.metric === 'cpu' && i.subject === 'System')!.current).toBe(`${MAC_CPU_TOTAL}%`);
    expect(items.find((i) => i.metric === 'memory')!.current).toBe(`${MAC_MEMORY_PERCENT}%`);
    expect(items.find((i) => i.metric === 'diskWrite')!.current).toBe(formatRate((await b.liveUpdate('summary')).system!.diskIo.writeBps));
    const tray = JSON.stringify(await b.popoverValues());
    expect(tray).toContain(`"cpuPercent":${MAC_CPU_TOTAL}`);
    expect(tray).toContain(`"memoryUsed":${MAC_MEMORY_USED}`);
  });
});

describe('memory has one total', () => {
  it('the live readings and the inventory say the same size for the Mac\'s unified memory', async () => {
    const b = make();
    const total = b.liveUpdate('summary').system!.memory.total;
    const layout = (await b.inventory()).memory;
    const unified = 'unified' in layout ? (layout as { unified: { total: number } }).unified.total : -1;
    expect(unified).toBe(total);
    expect(total).toBe(MAC_MEMORY_TOTAL);
  });
});

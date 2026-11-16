import { formatGhz, formatRate, formatStorage } from '../shared/format';
import { readValue } from './measured';
import { MockBackend, type MockOptions } from './mock-backend';
import { dataDisk, startupDisk } from './mock/disk-facts';
import { LINUX_MEMORY_TOTAL } from './mock/linux-machine';
import { WIN_MEMORY_TOTAL } from './mock/windows-machine';
import {
  MAC_BUILD_CPU,
  MAC_BUILD_DISK_BPS,
  MAC_CARGO_CPU,
  MAC_CPU_CLOCK_MHZ,
  MAC_CPU_MAX_MHZ,
  MAC_CPU_TEMP_C,
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
    expect(live.system!.cpu.frequencyMhz).toBe(MAC_CPU_CLOCK_MHZ);
    expect(live.system!.cpu.baseFrequencyMhz).toBeLessThan(MAC_CPU_CLOCK_MHZ);
    const sensor = live.system!.temperatures;
    expect('available' in sensor ? sensor.available[0]!.celsius : -1).toBe(MAC_CPU_TEMP_C);
  });

  it('Throttling and Is this normal say the same clock and temperature as the live readings', async () => {
    const b = make();
    const reading = (await b.throttling()).reading.evidence;
    expect(reading.find((e) => e.label === 'Clock')!.value).toBe(`${formatGhz(MAC_CPU_CLOCK_MHZ)} of ${formatGhz(MAC_CPU_MAX_MHZ)}`);
    expect(reading.find((e) => e.label === 'Temperature')!.value).toBe(`${MAC_CPU_TEMP_C} °C`);
    const inputs = (await b.throttling()).inputs;
    expect(inputs.find((i) => i.label === 'CPU clock speed')!.detail).toBe(`${formatGhz(MAC_CPU_CLOCK_MHZ)} of ${formatGhz(MAC_CPU_MAX_MHZ)}`);
    expect(inputs.find((i) => i.label === 'Temperature')!.detail).toBe(`CPU package at ${MAC_CPU_TEMP_C} °C`);
    expect((await b.baselines()).items.find((i) => i.metric === 'temperature')!.current).toBe(`${MAC_CPU_TEMP_C} °C`);
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

describe('the live history does not depend on when the page subscribed', () => {
  it('with ?stable the seconds before the current one are the same whatever second of the minute the page asks in', () => {
    const at = (offsetMs: number) => new MockBackend({ latencyMs: 0, now: () => NOW + offsetMs, os: 'macos', stableMs: 60_000 });
    const key = (b: MockBackend) => JSON.stringify(b.liveBacklog().map((u) => [u.system!.cpu.total, u.system!.diskIo.busy]));
    expect(key(at(7_000))).toBe(key(at(0)));
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

describe.each(['windows', 'linux'] as const)('the mock %s machine says one thing about its startup disk and its memory', (os) => {
  const sheet = startupDisk(os);
  const mount = os === 'windows' ? 'C:\\' : '/';

  it('the live readings, the inventory, the history, the weekly report and the Health reason agree on the disk', async () => {
    const b = make({ os });
    const live = b.liveUpdate('summary').system!.disks.find((d) => d.mount === mount)!;
    expect([live.total, live.free]).toEqual([sheet.total, sheet.free]);

    const inventory = await b.inventory();
    expect(inventory.disks[0]!.totalBytes).toBe(sheet.total);

    const trend = (await b.history('week', null)).diskTrends[0]!;
    expect(trend.total).toBe(sheet.total);
    expect(trend.points.at(-1)!.free).toBe(sheet.free);

    const report = (await b.weeklyReport(1)).disks[0]!;
    expect([report.total, report.free]).toEqual([sheet.total, sheet.free]);

    const reason = (await b.health()).reasons.find((r) => r.id === 'low-disk-c')!;
    expect(reason.title).toBe(`${sheet.name} is ${sheet.usedPercent}% full`);
    expect(reason.detail).toContain(sheet.freeText);
    const card = (await b.whySlow()).causes.find((c) => c.kind === 'lowDiskSpace')!;
    expect(card.title).toBe(`${sheet.name} is ${sheet.usedPercent}% full`);
    expect(card.evidence[0]!.value).toBe(`${formatStorage(sheet.free)} of ${formatStorage(sheet.total)}`);
    expect(JSON.stringify(await b.weeklyReport(1))).toContain(sheet.freeText);
  });

  it('the second disk is the same size in the live readings and the history', async () => {
    const b = make({ os });
    const data = dataDisk(os)!;
    const live = b.liveUpdate('summary').system!.disks[1]!;
    expect([live.total, live.free]).toEqual([data.total, data.free]);
    const trend = (await b.history('week', null)).diskTrends[1]!;
    expect(trend.total).toBe(data.total);
    expect(trend.points.at(-1)!.free).toBe(data.free);
  });

  it('the live readings and the inventory say the same memory size', async () => {
    const b = make({ os });
    const total = b.liveUpdate('summary').system!.memory.total;
    const layout = (await b.inventory()).memory;
    const installed = 'slots' in layout ? (layout as { slots: { modules: { capacityBytes: number }[] } }).slots.modules.reduce((a, m) => a + m.capacityBytes, 0) : -1;
    expect(installed).toBe(total);
    expect(total).toBe(os === 'windows' ? WIN_MEMORY_TOTAL : LINUX_MEMORY_TOTAL);
  });
});

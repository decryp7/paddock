import type { BootRecord, CoreBucket, DayCount, DiskTrend, History, HistoryPoint, HistoryRange, TimelineEvent } from '../api.types';
import { clamp, hash01, smoothNoise, wobble } from './noise';
import { round } from './processes';

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
const GB = 1024 ** 3;
const MB = 1024 ** 2;

export const RANGES: Record<HistoryRange, { span: number; step: number }> = {
  hour: { span: HOUR, step: 30_000 },
  day: { span: DAY, step: 5 * MIN },
  week: { span: 7 * DAY, step: 30 * MIN },
  month: { span: 30 * DAY, step: 2 * HOUR },
};

/** Whether a build is running at `at` (deterministic windows during working hours, plus the one running now). */
function building(at: number, now: number): boolean {
  if (now - at < 7 * MIN) return true;
  const d = new Date(at);
  const h = d.getHours() + d.getMinutes() / 60;
  const weekday = d.getDay() >= 1 && d.getDay() <= 5;
  if (!weekday || h < 9 || h > 19) return false;
  const slot = Math.floor(at / (20 * MIN));
  return hash01(slot, 11) > 0.72;
}

/** How busy the user typically is at `at`: 0 asleep … 1 working. */
function rhythm(at: number): number {
  const d = new Date(at);
  const h = d.getHours() + d.getMinutes() / 60;
  const weekday = d.getDay() >= 1 && d.getDay() <= 5;
  const work = h >= 8.5 && h <= 18.5 ? 1 : h > 18.5 && h < 23.5 ? 0.45 : 0.06;
  return weekday ? work : work * 0.5;
}

function point(at: number, now: number): HistoryPoint {
  const t = at / 1000;
  const r = rhythm(at);
  const build = building(at, now);
  const cpuBase = 2.5 + r * 16 + (build ? 58 : 0);
  const cpu = clamp(cpuBase * (1 + 0.35 * wobble(t, 1, 900)), 0.5, 99);
  const cpuMax = clamp(cpu + 8 + 30 * smoothNoise(t, 2, 600) * (r + 0.2), cpu, 100);
  // Memory climbs through the day (apps opened, Teams growing) and drops overnight when the laptop sleeps.
  const memory = clamp(34 + r * 30 + (build ? 6 : 0) + 4 * wobble(t, 3, 3600), 20, 96);
  const memoryMax = clamp(memory + 2 + 5 * smoothNoise(t, 4, 1200), memory, 99);
  const io = build ? 1 : 0.05 + r * 0.25;
  return {
    at,
    cpu: round(cpu, 1),
    cpuMin: round(clamp(cpu - 2 - 6 * smoothNoise(t, 13, 600), 0, cpu), 1),
    cpuMax: round(cpuMax, 1),
    memory: round(memory, 1),
    memoryMin: round(clamp(memory - 1.5, 0, memory), 1),
    memoryMax: round(memoryMax, 1),
    diskReadBps: Math.round((4 + 60 * io) * MB * (0.6 + 0.4 * smoothNoise(t, 5, 300))),
    diskWriteBps: Math.round((2 + 120 * io) * MB * (0.6 + 0.4 * smoothNoise(t, 6, 300))),
    netRxBps: Math.round((0.05 + r * 2.4) * MB * (0.4 + 1.2 * smoothNoise(t, 7, 200))),
    netTxBps: Math.round((0.02 + r * 0.6) * MB * (0.4 + 1.2 * smoothNoise(t, 8, 200))),
    gpu: round(clamp(3 + r * 9 + 6 * wobble(t, 9, 700), 0, 100), 1),
    temperature: round(clamp(42 + cpu * 0.45 + 3 * wobble(t, 10, 500), 35, 99), 1),
  };
}

/** Rule actions, cleanups, tuning, alerts and boots inside `[from, to]`, derived from the same build windows. */
function events(from: number, to: number, now: number, step: number): TimelineEvent[] {
  const out: TimelineEvent[] = [];
  const slotLen = 20 * MIN;
  // At wide ranges one marker per build window would bury the chart: keep the interesting ones.
  const every = step >= 2 * HOUR ? 6 : step >= 30 * MIN ? 3 : 1;
  let n = 0;
  for (let s = Math.floor(from / slotLen); s * slotLen <= to; s++) {
    const at = s * slotLen;
    if (at < from || !building(at + MIN, now) || building(at - slotLen + MIN, now)) continue;
    if (n++ % every !== 0) continue;
    out.push({ at: at + 5 * MIN, kind: 'throttle', label: 'Lowered priority of rustc.exe (Tame long CPU hogs)' });
    if (hash01(s, 12) > 0.5) out.push({ at: at + 10 * MIN, kind: 'cap', label: 'Capped rustc.exe at 50% CPU' });
    out.push({ at: at + 19 * MIN, kind: 'release', label: 'Released rustc.exe (below threshold)' });
  }
  // The build running now.
  if (now - 4 * MIN >= from && now - 4 * MIN <= to) out.push({ at: now - 4 * MIN, kind: 'throttle', label: 'Lowered priority of rustc.exe (Tame long CPU hogs)' });
  const fixed: TimelineEvent[] = [
    { at: now - 35 * MIN, kind: 'network', label: 'Joined Wi-Fi “Studio”' },
    { at: now - 58 * MIN, kind: 'network', label: 'VPN up (utun4)' },
    { at: now - 72 * MIN, kind: 'cleanup', label: 'Cleanup freed 4.6 GB' },
    { at: now - 3 * HOUR, kind: 'network', label: 'IP address on en0 changed: 192.168.1.23 → 192.168.1.41' },
    { at: now - 5 * HOUR, kind: 'tuning', label: 'Lighter visual effects' },
    { at: now - 27 * HOUR, kind: 'alert', label: 'Memory pressure high' },
    { at: now - 2 * DAY, kind: 'cleanup', label: 'Scheduled cleanup freed 6.1 GB' },
    { at: now - 52 * HOUR, kind: 'alert', label: 'Drive C: below 25% free' },
    { at: now - 4 * DAY, kind: 'alert', label: 'CPU reached 98 °C' },
    { at: now - 9 * DAY, kind: 'cleanup', label: 'Scheduled cleanup freed 8.3 GB' },
    { at: now - 12 * DAY, kind: 'boot', label: 'Windows started (boot 17.2 s)' },
    { at: now - 16 * DAY, kind: 'cleanup', label: 'Scheduled cleanup freed 5.4 GB' },
    { at: now - 19 * DAY, kind: 'boot', label: 'Windows started (boot 15.8 s)' },
    { at: now - 21 * DAY, kind: 'tuning', label: 'Best performance while plugged in' },
    { at: now - 26 * DAY, kind: 'boot', label: 'Windows started (boot 21.4 s)' },
  ];
  out.push(...fixed.filter((e) => e.at >= from && e.at <= to));
  return out.sort((a, b) => a.at - b.at);
}

/** C: loses ~5 GB a day to build output with scheduled cleanups giving some back; D: (media) barely moves. */
function diskTrends(now: number): DiskTrend[] {
  const cTotal = 953 * GB;
  const dTotal = 1863 * GB;
  const c: DiskTrend = { disk: 'C:', total: cTotal, points: [], forecast: { fullInDays: 41 } };
  const d: DiskTrend = { disk: 'D:', total: dTotal, points: [], forecast: 'notFilling' };
  for (let i = 30; i >= 0; i--) {
    const at = now - i * DAY;
    // Before each cleanup (further back) there was less free space than the straight line suggests.
    const freedSince = (i > 2 ? 6.1 : 0) + (i > 9 ? 8.3 : 0) + (i > 16 ? 5.4 : 0);
    const freeC = 210 + i * 5.1 - freedSince + (i > 0 ? 3 * wobble(at / 1000, 21, 3 * 86400) : 0);
    c.points.push({ at, free: Math.round(freeC * GB) });
    d.points.push({ at, free: Math.round((1204 + 2 * wobble(at / 1000, 22, 5 * 86400)) * GB) });
  }
  return [c, d];
}

function boots(now: number): BootRecord[] {
  const out: BootRecord[] = [];
  for (let i = 29; i >= 0; i--) {
    // Restarted only now and then; the laptop sleeps otherwise. Sign-ins after sleep count as "ready".
    const at = now - i * DAY - 14 * HOUR;
    const restarted = hash01(i, 31) > 0.62 || i === 12 || i === 19 || i === 26;
    out.push({
      at,
      bootSecs: restarted ? round(14 + 6 * hash01(i, 32) + (i < 10 ? 2.5 : 0), 1) : null,
      readySecs: round(22 + 18 * hash01(i, 33) + (29 - i) * 0.6, 1),
    });
  }
  return out;
}

function crashes(now: number): DayCount[] {
  const out: DayCount[] = [];
  const start = new Date(now);
  start.setHours(0, 0, 0, 0);
  for (let i = 29; i >= 0; i--) {
    const day = start.getTime() - i * DAY;
    const h = hash01(i, 41);
    out.push({
      day,
      crashes: i === 0 ? 6 : i === 1 ? 2 : h > 0.86 ? 2 : h > 0.7 ? 1 : 0,
      restarts: hash01(i, 42) > 0.75 || i === 12 || i === 19 || i === 26 ? 1 : 0,
    });
  }
  return out;
}

/** The mock history for `range` ending at `end` (or now). */
/** Per-core load around each point's CPU load: performance cores work harder than efficiency cores (mock only). */
function corePoints(points: readonly HistoryPoint[], cores: number, kinds: readonly string[]): CoreBucket[] {
  return points.map((p) => ({
    at: p.at,
    cores: Array.from({ length: cores }, (_, i) => {
      const w = kinds[i] === 'efficiency' ? 0.55 : kinds[i] === 'performance' ? 1.2 : 1;
      const avg = clamp(p.cpu * w * (0.65 + 0.7 * hash01(Math.floor(p.at / 60000) + i * 977, 21)), 0, 100);
      return { min: round(clamp(avg - 4 - 6 * hash01(i, p.at % 997), 0, avg), 1), avg: round(avg, 1), max: round(clamp(avg + 6 + 24 * hash01(i + 3, Math.floor(p.at / 1000) % 991), avg, 100), 1) };
    }),
  }));
}

/** The mock history for `range` ending at `end` (or now). */
export function mockHistory(range: HistoryRange, end: number | null, now: number, cores = 0, kinds: readonly string[] = []): History {
  const { span, step } = RANGES[range];
  const to = Math.floor((end ?? now) / step) * step;
  const from = to - span;
  const points: HistoryPoint[] = [];
  for (let at = from + step; at <= to; at += step) points.push(point(at, now));
  const consumersScale = range === 'hour' ? 1 : 0.55;
  return {
    range,
    from,
    to,
    resolutionMs: step,
    points,
    events: events(from, to, now, step),
    topConsumers: [
      { app: 'Rust build (cargo)', cpuAvg: round(range === 'hour' ? 31 : 14.2, 1), memoryAvg: Math.round((range === 'hour' ? 2.4 : 1.1) * GB) },
      { app: 'Google Chrome', cpuAvg: round(6.1 * (range === 'hour' ? 1 : 0.8), 1), memoryAvg: Math.round(4.1 * GB) },
      { app: 'WSL / Docker VM', cpuAvg: round(3.2, 1), memoryAvg: Math.round(4 * GB) },
      { app: 'Visual Studio Code', cpuAvg: round(3.0 * consumersScale + 1, 1), memoryAvg: Math.round(2.1 * GB) },
      { app: 'Microsoft Defender', cpuAvg: round(2.6, 1), memoryAvg: Math.round(370 * MB) },
      { app: 'Microsoft Teams', cpuAvg: round(1.8, 1), memoryAvg: Math.round(1.1 * GB) },
      { app: 'Windows services', cpuAvg: round(1.9, 1), memoryAvg: Math.round(1.3 * GB) },
    ],
    diskTrends: diskTrends(now),
    boots: boots(now),
    crashes: crashes(now),
    corePoints: cores > 0 ? corePoints(points, cores, kinds) : [],
  };
}

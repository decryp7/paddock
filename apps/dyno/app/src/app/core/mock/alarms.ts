import type { Alarm, AlarmChange, AlarmFilter, AlarmStats, BundlePreview, RecorderSample, RecorderSnapshot, RecorderSnapshotInfo } from '../api.types';
import { clamp, wobble } from './noise';

const MIN = 60_000;
const HOUR = 60 * MIN;
const GB = 1024 ** 3;

/** A few plausible alarms in different states, so the Alarms page has every kind of row to show. */
export function demoAlarms(now: number): Alarm[] {
  const base = {
    notes: [],
    acknowledgedAt: null,
    clearedAt: null,
    shelved: null,
    muted: false,
    chattering: false,
    expiresAt: null,
    metric: null,
    currentValue: null,
  } satisfies Partial<Alarm>;
  return [
    { ...base, id: 7, rule: 'network:newExposedListener', key: 'network:newExposedListener:node:3000', severity: 'warning', state: 'active', title: 'node started listening on TCP port 3000', detail: 'On every network interface: reachable from the network unless a firewall blocks it.', firstSeen: now - 4 * MIN, lastSeen: now - 4 * MIN, repeatCount: 1 },
    { ...base, id: 6, rule: 'threshold:gpu-load', key: 'threshold:gpu-load', severity: 'warning', state: 'active', title: 'GPU load is over its limit', detail: 'The value is 97.0; the limit is 90.0.', firstSeen: now - 22 * MIN, lastSeen: now - 1 * MIN, repeatCount: 14, currentValue: 97, metric: 'gpuLoad' as Alarm['metric'] },
    { ...base, id: 5, rule: 'alert:lowDisk', key: 'alert:lowDisk:Macintosh HD', severity: 'warning', state: 'acknowledged', title: 'Macintosh HD is 78% full', detail: '210 GB free of 953 GB.', firstSeen: now - 3 * HOUR, lastSeen: now - 40 * MIN, repeatCount: 3, acknowledgedAt: now - 2 * HOUR, notes: [{ at: now - 2 * HOUR, text: 'Waiting for the Xcode caches to be cleaned.' }], metric: 'diskFree' as Alarm['metric'] },
    { ...base, id: 4, rule: 'alert:crashLoop', key: 'alert:crashLoop:VirtualBox service keeps restarting', severity: 'critical', state: 'clearedUnacknowledged', title: 'VirtualBox service keeps restarting', detail: '6 restarts in 2 hours.', firstSeen: now - 5 * HOUR, lastSeen: now - 3 * HOUR, repeatCount: 6, clearedAt: now - 3 * HOUR },
    { ...base, id: 3, rule: 'alert:longHighCpu', key: 'alert:longHighCpu:rustc', severity: 'info', state: 'active', title: 'rustc has used over 40% CPU for 12 minutes', detail: 'A long build.', firstSeen: now - 50 * MIN, lastSeen: now - 12 * MIN, repeatCount: 2, shelved: { until: now + 40 * MIN } },
    { ...base, id: 2, rule: 'alert:overheating', key: 'alert:overheating:CPU', severity: 'critical', state: 'cleared', title: 'CPU reached 98 °C', detail: 'Thermal throttling for 3 minutes during a build with the lid closed.', firstSeen: now - 26 * HOUR, lastSeen: now - 26 * HOUR, repeatCount: 1, acknowledgedAt: now - 25 * HOUR, clearedAt: now - 25.5 * HOUR },
  ];
}

/** The engine's filter, for the mock only. */
export function filterAlarms(all: readonly Alarm[], f: AlarmFilter, now: number): Alarm[] {
  const rank = { info: 0, warning: 1, critical: 2 } as const;
  const wanted = f.states.length ? f.states : (['active', 'acknowledged', 'clearedUnacknowledged'] as const);
  return all
    .filter((a) => (wanted as readonly string[]).includes(a.state))
    .filter((a) => f.includeShelved || !isShelved(a, now))
    .filter((a) => !f.minSeverity || rank[a.severity] >= rank[f.minSeverity])
    .filter((a) => !f.rule || a.rule === f.rule)
    .sort((a, b) => b.lastSeen - a.lastSeen)
    .slice(0, f.limit > 0 ? f.limit : 100);
}

export function isShelved(a: Alarm, now: number): boolean {
  return a.shelved === 'untilReboot' || (a.shelved !== null && typeof a.shelved === 'object' && a.shelved.until > now);
}

export function demoStats(alarms: readonly Alarm[]): AlarmStats {
  return {
    open: alarms.filter((a) => a.state !== 'cleared').length,
    unacknowledged: alarms.filter((a) => a.state === 'active' || a.state === 'clearedUnacknowledged').length,
    chattering: [],
    noisiest: [
      { rule: 'threshold:gpu-load', occurrences: 14 },
      { rule: 'alert:crashLoop', occurrences: 6 },
      { rule: 'alert:lowDisk', occurrences: 3 },
    ],
    meanTimeToAcknowledgeMs: 18 * MIN,
    medianTimeToAcknowledgeMs: 11 * MIN,
    suggestions: [{ rule: 'threshold:gpu-load', kind: 'tune', message: 'GPU load fired 14 times in a day. Raise the limit or lengthen how long it must hold.' }],
  };
}

export function demoHistory(alarms: readonly Alarm[]): AlarmChange[] {
  return alarms
    .flatMap((a): AlarmChange[] => [
      { kind: 'raised', alarm: a, notify: false, at: a.firstSeen },
      ...(a.acknowledgedAt ? [{ kind: 'acknowledged' as const, alarm: a, notify: false, at: a.acknowledgedAt }] : []),
      ...(a.clearedAt ? [{ kind: 'returned' as const, alarm: a, notify: false, at: a.clearedAt }] : []),
    ])
    .sort((a, b) => b.at - a.at);
}

/** Ten minutes of one-second samples ending at `at`, with a CPU burst in the middle (a build) so a capture has a story. */
export function recorderSamples(at: number, seconds = 600): RecorderSample[] {
  const out: RecorderSample[] = [];
  const total = 48 * GB;
  for (let i = 0; i < seconds; i++) {
    const t = (at / 1000) - (seconds - i);
    const burst = i > seconds * 0.45 && i < seconds * 0.8 ? 1 : 0;
    const cpu = clamp(14 + 55 * burst + 9 * wobble(t, 31, 6), 2, 99);
    out.push({
      at: at - (seconds - i) * 1000,
      cpuTotal: Math.round(cpu * 10) / 10,
      cpuCores: Array.from({ length: 14 }, (_, c) => Math.round(clamp(cpu + 18 * wobble(t, 40 + c, 5), 0, 100))),
      memoryUsed: Math.round((11.5 + 2.2 * burst + 0.4 * wobble(t, 32, 40)) * GB),
      memoryTotal: total,
      diskReadBps: Math.round((6 + 80 * burst) * 1024 ** 2 * (0.6 + 0.4 * Math.abs(wobble(t, 33, 4)))),
      diskWriteBps: Math.round((3 + 140 * burst) * 1024 ** 2 * (0.6 + 0.4 * Math.abs(wobble(t, 34, 4)))),
      netRxBps: Math.round((0.2 + 1.8 * Math.abs(wobble(t, 35, 9))) * 1024 ** 2),
      netTxBps: Math.round((0.05 + 0.4 * Math.abs(wobble(t, 36, 9))) * 1024 ** 2),
      top: [
        { name: burst ? 'rustc' : 'Google Chrome', cpu: Math.round((burst ? 38 : 6) * 10) / 10, memory: Math.round((burst ? 2.4 : 4.1) * GB) },
        { name: 'Visual Studio Code', cpu: 3.1, memory: Math.round(2.1 * GB) },
        { name: 'WindowServer', cpu: 2.4, memory: Math.round(0.3 * GB) },
      ],
    });
  }
  return out;
}

export function demoSnapshotInfos(now: number): RecorderSnapshotInfo[] {
  return [
    { id: 3, at: now - 22 * MIN, reason: { alarm: { alarmId: 6, title: 'GPU load is over its limit' } }, bytes: 412_000, samples: 600 },
    { id: 2, at: now - 3 * HOUR, reason: 'manual', bytes: 398_000, samples: 600 },
    { id: 1, at: now - 26 * HOUR, reason: { alarm: { alarmId: 2, title: 'CPU reached 98 °C' } }, bytes: 405_000, samples: 600 },
  ];
}

export function demoSnapshot(info: RecorderSnapshotInfo): RecorderSnapshot {
  return { version: 1, at: info.at, reason: info.reason, samples: recorderSamples(info.at, info.samples) };
}

export function demoBundlePreview(): BundlePreview {
  const files = [
    { path: 'README.txt', description: 'What this is', bytes: 520 },
    { path: 'system/inventory.json', description: 'Operating system, processor, memory slots and disks', bytes: 4_812 },
    { path: 'settings.json', description: "Dyno's settings", bytes: 2_240 },
    { path: 'alarms/open.json', description: 'Alarms', bytes: 6_100 },
    { path: 'alarms/stats.json', description: 'Alarm statistics', bytes: 840 },
    { path: 'alarms/history.json', description: 'Recent alarm changes', bytes: 18_400 },
    { path: 'network/sockets.json', description: 'Listening ports and connections (addresses redacted)', bytes: 9_300 },
    { path: 'activity.json', description: 'What Dyno recorded recently', bytes: 11_900 },
    { path: 'alerts.json', description: 'Recent alerts', bytes: 3_100 },
    { path: 'recorder/last-10-minutes.json', description: 'The last ten minutes at one-second resolution', bytes: 412_000 },
    { path: 'manifest.json', description: 'Every file and what was removed', bytes: 1_900 },
  ];
  return { files, totalBytes: files.reduce((a, f) => a + f.bytes, 0), redactions: { homePaths: 12, profilePaths: 3, machineNames: 5, userNames: 9, emails: 0, ipAddresses: 41 } };
}

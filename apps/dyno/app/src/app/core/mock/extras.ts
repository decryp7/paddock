import type {
  BootReport,
  Change,
  ChangeTimeline,
  ChangesQuery,
  DuplicateGroup,
  FinderQuery,
  FinderResult,
  FoundFile,
  GpuReport,
  MaintenanceSettings,
  MaintenanceStatus,
  SleepReport,
  SoftwareInventory,
  UsageReport,
  Weekday,
} from '../api.types';

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
const MB = 1024 ** 2;
const GB = 1024 ** 3;

/** The default window of "What changed" when the query says 0 (the engine's default). */
const DEFAULT_HOURS = 72;

const CHANGES: { ago: number; kind: Change['kind']; title: string; detail: string }[] = [
  { ago: 25 * MIN, kind: 'updated', title: 'Updated Visual Studio Code', detail: '1.112.0 to 1.113.1' },
  { ago: 5 * HOUR, kind: 'startupAdded', title: 'New startup item Zoom Opener', detail: 'com.zoom.ZoomOpener' },
  { ago: 9 * HOUR + 40 * MIN, kind: 'installed', title: 'Installed Figma', detail: 'Version 126.4.2' },
  { ago: DAY + 3 * HOUR, kind: 'updated', title: 'Updated Docker Desktop', detail: '4.62.0 to 4.63.2' },
  { ago: DAY + 3 * HOUR + 4 * MIN, kind: 'updated', title: 'Updated Node.js', detail: '24.9.0 to 24.10.1' },
  { ago: DAY + 8 * HOUR, kind: 'startupRemoved', title: 'Startup item removed: Dropbox Updater', detail: 'com.dropbox.DropboxUpdater' },
  { ago: DAY + 8 * HOUR + 12 * MIN, kind: 'removed', title: 'Removed Dropbox', detail: 'Was version 235.4.4' },
  { ago: 2 * DAY + 2 * HOUR, kind: 'installed', title: 'Installed Linear', detail: 'Version 1.28.0' },
  { ago: 2 * DAY + 6 * HOUR, kind: 'updated', title: 'Updated Google Chrome', detail: '155.0.7632.76 to 156.0.7712.48' },
  { ago: 2 * DAY + 6 * HOUR + 3 * MIN, kind: 'startupAdded', title: 'New startup item Google Updater', detail: 'com.google.keystone.agent' },
  { ago: 3 * DAY + HOUR, kind: 'installed', title: 'Installed Homebrew package ripgrep', detail: 'Version 15.1.0' },
  { ago: 4 * DAY + 5 * HOUR, kind: 'updated', title: 'Updated Slack', detail: '4.41.98 to 4.41.105' },
  { ago: 6 * DAY + 2 * HOUR, kind: 'installed', title: 'Installed Rectangle', detail: 'Version 0.88' },
];

/** What changed in the window the query asks for (newest first), and since when Dyno has been watching. */
export function mockChanges(query: ChangesQuery, now: number): ChangeTimeline {
  const hours = query.hours === 0 ? DEFAULT_HOURS : Math.min(query.hours, 24 * 30);
  const to = query.around ? Math.min(query.around + hours * HOUR, now) : now;
  const from = (query.around ?? now) - hours * HOUR;
  const changes = CHANGES.map((c) => ({ at: now - c.ago, kind: c.kind, title: c.title, detail: c.detail })).filter((c) => c.at >= from && c.at <= to);
  return { from, to, changes, watchingSince: now - 9 * DAY };
}

export function mockSleep(): SleepReport {
  return {
    blockers: [
      { name: 'zoom.us', pid: 4821, prevents: 'displaySleep', reason: 'Video call in progress', heldFor: '00:42:17' },
      { name: 'coreaudiod', pid: 412, prevents: 'sleep', reason: 'Audio playing', heldFor: '01:12:03' },
      { name: 'Backblaze Backup', pid: 903, prevents: 'sleep', reason: 'Uploading a backup', heldFor: '00:06:51' },
    ],
    wakes: [
      { when: '2026-10-22 08:01:12 +0800', reason: 'Lid open' },
      { when: '2026-10-22 03:14:40 +0800', reason: 'Power Nap: network maintenance' },
      { when: '2026-10-21 18:22:05 +0800', reason: 'Keyboard or mouse' },
      { when: '2026-10-21 07:58:31 +0800', reason: 'Lid open' },
      { when: '2026-10-20 22:47:09 +0800', reason: 'Bluetooth: Magic Trackpad' },
    ],
    blockersUnavailable: null,
    wakesUnavailable: null,
  };
}

export function mockBoot(now: number): BootReport {
  const secs = [47.2, 52.8, 44.1, 49.6, 71.4, 46.3, 45.9, 50.2];
  return {
    boots: secs.map((s, i) => ({ at: now - (i * 2 + 0.4) * DAY - 6 * HOUR, bootSecs: s, readySecs: Math.round((s + 18 + (i % 3) * 4) * 10) / 10 })),
    slowest: [
      { name: 'com.adobe.AdobeCreativeCloud', secs: 9.4 },
      { name: 'Docker Desktop (com.docker.helper)', secs: 7.9 },
      { name: 'com.microsoft.OneDrive.Updater', secs: 5.2 },
      { name: 'Little Snitch Daemon', secs: 3.1 },
      { name: 'com.zoom.ZoomOpener', secs: 2.4 },
    ],
    lastTotalSecs: 47.2,
    verdict: 'The last start-up took 47 s, about the same as usual (median 48 s).',
    stepsUnavailable: null,
  };
}

export function mockUsage(): UsageReport {
  return {
    apps: [
      { app: 'Google Chrome', bytesIn: 4.8 * GB, bytesOut: 612 * MB, energyImpact: 38.4 },
      { app: 'Slack', bytesIn: 1.1 * GB, bytesOut: 302 * MB, energyImpact: 12.6 },
      { app: 'zoom.us', bytesIn: 0.9 * GB, bytesOut: 0.7 * GB, energyImpact: 21.3 },
      { app: 'Dropbox', bytesIn: 640 * MB, bytesOut: 1.9 * GB, energyImpact: 3.1 },
      { app: 'Spotify', bytesIn: 512 * MB, bytesOut: 18 * MB, energyImpact: 4.4 },
      { app: 'Docker Desktop', bytesIn: 388 * MB, bytesOut: 74 * MB, energyImpact: 15.2 },
      { app: 'Xcode', bytesIn: 241 * MB, bytesOut: 12 * MB, energyImpact: 27.9 },
      { app: 'Mail', bytesIn: 96 * MB, bytesOut: 22 * MB, energyImpact: 1.8 },
      { app: 'Visual Studio Code', bytesIn: 54 * MB, bytesOut: 9 * MB, energyImpact: 9.7 },
      { app: 'WindowServer', bytesIn: 0, bytesOut: 0, energyImpact: 14.5 },
    ].map((a) => ({ ...a, bytesIn: Math.round(a.bytesIn), bytesOut: Math.round(a.bytesOut) })),
    networkUnavailable: null,
    energyUnavailable: null,
  };
}

export function mockGpu(): GpuReport {
  return {
    processes: [
      { pid: 311, name: 'WindowServer', gpuPercent: 14.2, memoryBytes: Math.round(612 * MB) },
      { pid: 5120, name: 'Google Chrome Helper (GPU)', gpuPercent: 9.8, memoryBytes: Math.round(388 * MB) },
      { pid: 4821, name: 'zoom.us', gpuPercent: 6.1, memoryBytes: Math.round(204 * MB) },
      { pid: 7788, name: 'Figma', gpuPercent: 3.4, memoryBytes: Math.round(176 * MB) },
      { pid: 622, name: 'Dock', gpuPercent: 0.7, memoryBytes: Math.round(42 * MB) },
    ],
    source: 'Apple GPU accelerator counters',
    unavailable: null,
  };
}

const SOFTWARE: [string, string, string][] = [
  ['1Password', '8.11.14', 'AgileBits Inc.'],
  ['Activity Monitor', '11.0', 'Apple'],
  ['Adobe Creative Cloud', '6.7.0.278', 'Adobe Inc.'],
  ['Alfred', '5.7.1', 'Running with Crayons Ltd'],
  ['App Store', '3.0', 'Apple'],
  ['Brave Browser', '1.84.132', 'Brave Software'],
  ['Calculator', '11.0', 'Apple'],
  ['Calendar', '15.0', 'Apple'],
  ['Docker Desktop', '4.63.2', 'Docker Inc.'],
  ['Dropbox', '237.4.1', 'Dropbox, Inc.'],
  ['Figma', '126.4.2', 'Figma, Inc.'],
  ['Finder', '16.0', 'Apple'],
  ['Firefox', '149.0.2', 'Mozilla'],
  ['GitHub Desktop', '3.5.4', 'GitHub, Inc.'],
  ['Google Chrome', '156.0.7712.48', 'Google LLC'],
  ['iTerm', '3.6.2', 'George Nachman'],
  ['Keynote', '15.1', 'Apple'],
  ['Linear', '1.28.0', 'Linear Orbit, Inc.'],
  ['Mail', '17.0', 'Apple'],
  ['Messages', '15.0', 'Apple'],
  ['Microsoft Excel', '16.104', 'Microsoft Corporation'],
  ['Microsoft OneDrive', '26.180.0911', 'Microsoft Corporation'],
  ['Microsoft Word', '16.104', 'Microsoft Corporation'],
  ['Notes', '5.0', 'Apple'],
  ['Notion', '4.18.0', 'Notion Labs, Inc.'],
  ['Numbers', '15.1', 'Apple'],
  ['Obsidian', '1.9.14', 'Obsidian'],
  ['Pages', '15.1', 'Apple'],
  ['Photos', '11.0', 'Apple'],
  ['Preview', '12.0', 'Apple'],
  ['Rectangle', '0.88', 'Ryan Hanson'],
  ['Safari', '27.0', 'Apple'],
  ['Slack', '4.41.105', 'Slack Technologies, LLC'],
  ['Spotify', '1.2.78.404', 'Spotify AB'],
  ['Sublime Text', '4.0.4200', 'Sublime HQ Pty Ltd'],
  ['System Settings', '27.0', 'Apple'],
  ['Terminal', '2.15', 'Apple'],
  ['VLC', '3.0.23', 'VideoLAN'],
  ['Visual Studio Code', '1.113.1', 'Microsoft Corporation'],
  ['Xcode', '27.0', 'Apple'],
  ['zoom.us', '6.6.2', 'Zoom Video Communications, Inc.'],
];

export function mockSoftware(): SoftwareInventory {
  return { items: SOFTWARE.map(([name, version, publisher]) => ({ name, version, publisher })), truncated: false, unavailable: null };
}

// ----------------------------------------------------------------------------------------------- file finder

const file = (id: string, path: string, bytes: number, modifiedAgo: number, now: number): FoundFile => ({ id, path, bytes: Math.round(bytes), modified: now - modifiedAgo });

/** The finder's answer for a folder: about eight big files and three groups of duplicates. */
export function mockFinder(query: FinderQuery, now: number): FinderResult {
  const root = query.folder.replace(/\/+$/, '');
  const f = (id: string, rel: string, bytes: number, ago: number) => file(id, `${root}/${rel}`, bytes, ago, now);
  const large = [
    f('l1', 'design-assets/brand-video-final.mov', 6.4 * GB, 41 * DAY),
    f('l2', 'ml-experiments/datasets/images-v3.tar', 4.1 * GB, 88 * DAY),
    f('l3', 'android-app/build/outputs/app-release.aab', 1.9 * GB, 12 * DAY),
    f('l4', 'backups/laptop-2025-12.dmg', 1.6 * GB, 300 * DAY),
    f('l5', 'podcast/episode-42-raw.wav', 1.2 * GB, 17 * DAY),
    f('l6', 'vm-images/ubuntu-24.04-dev.qcow2', 980 * MB, 150 * DAY),
    f('l7', 'ios-app/DerivedData-export.zip', 740 * MB, 9 * DAY),
    f('l8', 'docs/conference-recording.mp4', 512 * MB, 62 * DAY),
  ];
  const dup = (each: number, copies: [string, string, number][]): DuplicateGroup => ({
    bytesEach: Math.round(each),
    wastedBytes: Math.round(each * (copies.length - 1)),
    files: copies.map(([id, rel, ago]) => f(id, rel, each, ago)),
  });
  const duplicates = [
    dup(1.4 * GB, [
      ['d1a', 'archive/old-projects/site-redesign.psd', 210 * DAY],
      ['d1b', 'design-assets/site-redesign.psd', 95 * DAY],
      ['d1c', 'Downloads-sorted/site-redesign (1).psd', 30 * DAY],
    ]),
    dup(420 * MB, [
      ['d2a', 'ml-experiments/models/resnet50.onnx', 120 * DAY],
      ['d2b', 'android-app/assets/models/resnet50.onnx', 118 * DAY],
    ]),
    dup(86 * MB, [
      ['d3a', 'docs/slides/quarterly-review.pdf', 44 * DAY],
      ['d3b', 'docs/shared/quarterly-review.pdf', 44 * DAY - HOUR],
      ['d3c', 'docs/Q3/quarterly-review-final.pdf', 20 * DAY],
    ]),
  ];
  const keep = (x: FoundFile) => x.bytes >= query.minBytes;
  const wantLarge = query.mode !== 'duplicates';
  const wantDup = query.mode !== 'large';
  const dups = wantDup ? duplicates.filter((g) => g.bytesEach >= query.minBytes) : [];
  return {
    scanId: 'scan-1',
    folder: root,
    large: wantLarge ? large.filter(keep) : [],
    duplicates: dups,
    scanned: 18_342,
    unreadable: 0,
    limit: null,
    wastedBytes: dups.reduce((a, g) => a + g.wastedBytes, 0),
  };
}

// ---------------------------------------------------------------------------------------------- maintenance

const WEEKDAYS: Weekday[] = ['sun', 'mon', 'tue', 'wed', 'thu', 'fri', 'sat'];

/** Where the windows stand at `now` (local time), as the engine's `MaintenanceSettings::status`. */
export function mockMaintenanceStatus(s: MaintenanceSettings, now: number): MaintenanceStatus {
  if (!s.enabled) return { enabled: false, openNow: true, closesAt: null, nextOpensAt: null };
  let closesAt: number | null = null;
  let next: number | null = null;
  const base = new Date(now);
  base.setHours(0, 0, 0, 0);
  for (let d = -1; d <= 8; d++) {
    const midnight = new Date(base);
    midnight.setDate(base.getDate() + d);
    for (const w of s.windows) {
      if (!w.days.includes(WEEKDAYS[midnight.getDay()]!)) continue;
      const start = midnight.getTime() + w.startMinute * MIN;
      const len = (w.endMinute - w.startMinute + 1440) % 1440;
      const end = start + len * MIN;
      if (start <= now && now < end) closesAt = Math.max(closesAt ?? 0, end);
      else if (start > now) next = Math.min(next ?? Infinity, start);
    }
  }
  return { enabled: true, openNow: closesAt !== null, closesAt, nextOpensAt: closesAt === null ? next : null };
}

/** The same checks as the engine's `MaintenanceSettings::validate`, with its sentences. */
export function mockMaintenanceProblem(s: MaintenanceSettings): string | null {
  if (s.windows.length > 14) return 'At most 14 maintenance windows.';
  if (s.enabled && s.windows.length === 0) return 'Add a window, or turn maintenance windows off.';
  for (const w of s.windows) {
    if (w.days.length === 0) return 'Pick at least one day for each window.';
    if (w.startMinute < 0 || w.startMinute >= 1440 || w.endMinute < 0 || w.endMinute >= 1440) return "A window's times are between 00:00 and 23:59.";
    if (w.startMinute === w.endMinute) return 'A window must end at a different time than it starts.';
  }
  return null;
}

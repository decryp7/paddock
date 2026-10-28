import type {
  DueFile,
  SearchSeed,
  StartupEntry,
  StartupFilter,
  StartupOverview,
  SweepPlan,
  WatchChange,
  WatchFilter,
  WatchOverview,
  WatchPreview,
  WatchTemplateInfo,
  WatchedFolder,
  WatchedFolderView,
} from '../api.types';

// The mock side of right-click search, hash search, the startup filter and watched folders. It mimics the engine's contract
// with plausible data; the engine (`dyno_core::weblinks`, `startup`, `watch`) is the reference for behaviour.

const MB = 1000 * 1000;
const MIN = 60_000;
const DAY = 24 * 60 * MIN;

/** The user name, computer name and home folder the mock pretends to run as (what the redaction removes). */
export const MOCK_IDENTITY = { user: 'alex', machine: 'alexs-macbook', home: '/Users/alex' };

/** A simple stand-in for the engine's redaction: home folder, names, IP addresses and GUIDs are removed. */
export function mockSearchSeed(text: string): SearchSeed {
  const removed: string[] = [];
  let out = text;
  const strip = (re: RegExp, one: string, many: string, to = ' '): void => {
    const n = (out.match(re) ?? []).length;
    if (n > 0) removed.push(`${n} ${n === 1 ? one : many}`);
    out = out.replace(re, to);
  };
  strip(new RegExp(`${MOCK_IDENTITY.home}/`, 'g'), 'home folder', 'home folders');
  strip(new RegExp(`\\b${MOCK_IDENTITY.user}\\b`, 'gi'), 'user name', 'user names');
  strip(new RegExp(`\\b${MOCK_IDENTITY.machine}\\b`, 'gi'), 'computer name', 'computer names');
  strip(/\b\d{1,3}(\.\d{1,3}){3}\b/g, 'IP address', 'IP addresses');
  strip(/\{?[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\}?/gi, 'GUID', 'GUIDs');
  return { text: out.split(/\s+/).filter(Boolean).join(' ').slice(0, 200), removed };
}

/** The address the engine would build (DuckDuckGo, query encoded). */
export function mockSearchUrl(query: string): string {
  const q = query.split(/\s+/).filter(Boolean).join(' ');
  if (!q) throw new Error('Type something to search for.');
  if (q.length > 200) throw new Error('The search text may have at most 200 characters.');
  return `https://duckduckgo.com/?q=${encodeURIComponent(q).replace(/%20/g, '+')}`;
}

/** A stable, plausible SHA-256 for a path (not a real hash). */
export function mockHash(path: string): string {
  let h = 0x811c9dc5;
  let out = '';
  for (let round = 0; round < 8; round++) {
    for (let i = 0; i < path.length; i++) h = Math.imul(h ^ (path.charCodeAt(i) + round), 0x01000193) >>> 0;
    out += h.toString(16).padStart(8, '0');
  }
  return out;
}

const norm = (p: string) => p.replace(/\\/g, '/').toLowerCase();

/** Whether the OS owns the entry (the Windows rules of the engine; the mock's startup list is a Windows one). */
export function mockIsOs(e: StartupEntry): boolean {
  const exe = norm(e.exe ?? '');
  return /^[a-z]:\/windows\//.test(exe) || norm(e.name).startsWith('/microsoft/windows/') || e.publisher === 'Microsoft Windows';
}

/** The startup list after the filter, with counts that do not depend on it. */
export function mockStartupOverview(all: StartupEntry[], filter: StartupFilter): StartupOverview {
  const kind = (e: StartupEntry) => (mockIsOs(e) ? 'os' : e.publisher?.trim() ? 'signed' : 'other');
  const entries = all.filter((e) => !((kind(e) === 'os' && filter.hideOs) || (kind(e) === 'signed' && filter.hideSigned)));
  return {
    entries,
    total: all.length,
    osCount: all.filter((e) => kind(e) === 'os').length,
    signedCount: all.filter((e) => kind(e) === 'signed').length,
  };
}

const TEMPLATES: (WatchTemplateInfo & { filter: WatchFilter })[] = [
  {
    id: 'installers',
    name: 'Downloads: installers',
    about: 'Disk images, installers and packages you downloaded. Pick your Downloads folder.',
    keepForSecs: 7 * 86400,
    recursive: false,
    filterText: '.dmg, .pkg, .exe, .msi, .deb, .rpm, .appimage, .iso',
    filter: { extensions: ['dmg', 'pkg', 'exe', 'msi', 'deb', 'rpm', 'appimage', 'iso'], nameStarts: [] },
  },
  {
    id: 'screenshots',
    name: 'Screenshots',
    about: 'Files that start like a screenshot. Pick the folder your screenshots go to.',
    keepForSecs: 3 * 86400,
    recursive: false,
    filterText: 'Screenshot…, Screen Shot…, Screen Recording…, Capture…',
    filter: { extensions: [], nameStarts: ['Screenshot', 'Screen Shot', 'Screen Recording', 'Capture'] },
  },
  {
    id: 'build-output',
    name: 'Build output',
    about: 'Every file in a build or scratch folder, subfolders included. Pick the folder yourself, never a source folder.',
    keepForSecs: 2 * 86400,
    recursive: true,
    filterText: 'every file',
    filter: { extensions: [], nameStarts: [] },
  },
];

/** Folders the mock's "choose a folder" dialog hands out in turn. */
const CHOICES = [
  '/Users/alex/Downloads',
  '/Users/alex/Pictures/Screenshots',
  '/Users/alex/dev/scratch/build',
  '/Users/alex/Movies/Exports',
  '/Users/alex/dev/tmp',
  '/Users/alex/Pictures/Exports',
];

const FILES: [string, number, number][] = [
  ['Xcode_16.2.dmg', 2400, 12],
  ['zoom-installer.pkg', 190, 9],
  ['Slack-4.41.dmg', 180, 8.5],
  ['node-v22.pkg', 75, 8],
  ['Figma.dmg', 140, 7.5],
  ['setup-1.4.2.exe', 64, 7.2],
];

/** The state behind the mock's watched folders. */
export class MockWatch {
  private folders: WatchedFolder[] = [];
  private counter = 0;

  constructor(private readonly now: () => number) {
    const t = this.now();
    this.folders = [
      this.make('installers', '/Users/alex/Downloads', 7 * 86400, false, t - 9 * DAY, true, true, {
        at: t - 40 * MIN,
        moved: 3,
        bytes: 2_650 * MB,
        failed: 0,
        firstFailure: null,
        quarantineId: 'w-demo-1',
      }),
      this.make('screenshots', '/Users/alex/Pictures/Screenshots', 3 * 86400, false, t - 20 * DAY, true, true, {
        at: t - 5 * MIN,
        moved: 0,
        bytes: 0,
        failed: 0,
        firstFailure: null,
        quarantineId: null,
      }),
      this.make('build-output', '/Users/alex/dev/scratch/build', 2 * 86400, true, t - 2 * DAY, false, false, null),
    ];
  }

  private make(
    template: string | null,
    path: string,
    keepForSecs: number,
    recursive: boolean,
    added: number,
    enabled: boolean,
    previewed: boolean,
    lastSweep: WatchedFolder['lastSweep'],
  ): WatchedFolder {
    const filter = TEMPLATES.find((t) => t.id === template)?.filter ?? { extensions: [], nameStarts: [] };
    return { id: `w-${++this.counter}`, path, keepForSecs, filter, recursive, enabled, previewed, template, added, lastSweep };
  }

  private describe(f: WatchFilter): string {
    const parts = [...f.extensions.map((e) => `.${e}`), ...f.nameStarts.map((p) => `${p}…`)];
    return parts.length === 0 ? 'every file' : parts.join(', ');
  }

  overview(): WatchOverview {
    const every = (f: WatchedFolder) => Math.min(15 * MIN, Math.max(MIN, f.keepForSecs * 100));
    const views: WatchedFolderView[] = this.folders.map((folder) => ({
      folder: structuredClone(folder),
      exists: true,
      filterText: this.describe(folder.filter),
      nextSweep: folder.enabled ? (folder.lastSweep?.at ?? folder.added) + every(folder) : null,
    }));
    return { folders: views, templates: TEMPLATES.map(({ filter: _f, ...t }) => ({ ...t })) };
  }

  add(template: string | null, keepForSecs: number, recursive: boolean): WatchOverview {
    if (keepForSecs < 600 || keepForSecs > 864_000) throw new Error('Choose a keep-for time between 10 minutes and 10 days.');
    if (this.folders.length >= 20) throw new Error('At most 20 folders can be watched; stop watching one first.');
    // The OS folder dialog: the mock "chooses" the first folder that is not watched yet.
    const path = CHOICES.find((c) => !this.folders.some((f) => c.startsWith(f.path) || f.path.startsWith(c)));
    if (!path) throw new Error('Every folder the demo offers is already watched, and watched folders may not overlap.');
    this.folders.push(this.make(template, path, keepForSecs, recursive, this.now(), false, false, null));
    return this.overview();
  }

  private find(id: string): WatchedFolder {
    const f = this.folders.find((x) => x.id === id);
    if (!f) throw new Error('Dyno no longer watches that folder.');
    return f;
  }

  change(id: string, c: WatchChange): WatchOverview {
    const f = this.find(id);
    if (c.keepForSecs !== undefined) {
      if (c.keepForSecs < 600 || c.keepForSecs > 864_000) throw new Error('Choose a keep-for time between 10 minutes and 10 days.');
      if (c.keepForSecs < f.keepForSecs) {
        f.previewed = false;
        f.enabled = false;
      }
      f.keepForSecs = c.keepForSecs;
    }
    if (c.recursive !== undefined) {
      if (c.recursive && !f.recursive) {
        f.previewed = false;
        f.enabled = false;
      }
      f.recursive = c.recursive;
    }
    if (c.enabled !== undefined) {
      if (c.enabled && !f.previewed) throw new Error('Look at the preview first: it shows exactly what would be moved.');
      f.enabled = c.enabled;
    }
    return this.overview();
  }

  remove(id: string): WatchOverview {
    this.find(id);
    this.folders = this.folders.filter((f) => f.id !== id);
    return this.overview();
  }

  private plan(f: WatchedFolder): SweepPlan {
    const t = this.now();
    const keepDays = f.keepForSecs / 86400;
    const due: DueFile[] = FILES.filter(([, , age]) => age >= keepDays)
      .filter(([name]) => this.matches(f.filter, name))
      .map(([name, mb, age]) => ({ path: `${f.path}/${name}`, bytes: mb * MB, arrived: t - age * DAY, modified: t - age * DAY }));
    due.sort((a, b) => a.arrived - b.arrived);
    return {
      due,
      dueTotal: due.length,
      dueBytes: due.reduce((s, d) => s + d.bytes, 0),
      waiting: FILES.length - due.length,
      other: 14,
      links: 0,
      refused: 0,
      truncated: false,
    };
  }

  private matches(f: WatchFilter, name: string): boolean {
    if (f.extensions.length === 0 && f.nameStarts.length === 0) return true;
    const lower = name.toLowerCase();
    const ext = lower.includes('.') ? lower.slice(lower.lastIndexOf('.') + 1) : '';
    return f.extensions.includes(ext) || f.nameStarts.some((p) => lower.startsWith(p.toLowerCase()));
  }

  preview(id: string): WatchPreview {
    const f = this.find(id);
    f.previewed = true;
    return { id, plan: this.plan(f) };
  }

  sweep(id: string): WatchOverview {
    const f = this.find(id);
    if (!f.previewed) throw new Error('Look at the preview first: it shows exactly what would be moved.');
    const p = this.plan(f);
    f.lastSweep = { at: this.now(), moved: p.dueTotal, bytes: p.dueBytes, failed: 0, firstFailure: null, quarantineId: p.dueTotal > 0 ? `w-demo-${f.id}` : null };
    return this.overview();
  }
}

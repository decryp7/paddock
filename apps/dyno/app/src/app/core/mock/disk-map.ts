/**
 * Mock data for the disk map: three volumes (internal, external, network) with invented content (user `alex`, server
 * `nas.local`), a size tree, and a scan "job" that reports progress through status events. It mimics the contract of the
 * engine (`DiskScanStatus` events, one level at a time with an "other" bucket); it is never the reference for behaviour.
 */
import type {
  CommandError,
  DiskChildren,
  DiskChildrenQuery,
  DiskNode,
  DiskScanStatus,
  DiskTarget,
  Folded,
  NodeKind,
  ScanState,
  Volume,
} from '../api.types';

const GB = 1024 ** 3;
const MB = 1024 ** 2;

/** The volumes the mock lists. */
export function mockVolumes(): Volume[] {
  const base = { rotational: false, readOnly: false, reachable: true, wholeVolume: true };
  return [
    { ...base, id: '/', name: 'Macintosh HD', mount: '/', fs: 'apfs', location: 'internal', capacity: Math.round(994.7 * GB), free: Math.round(378.2 * GB) },
    {
      ...base,
      id: '/Volumes/Photos',
      name: 'Photos',
      mount: '/Volumes/Photos',
      fs: 'exfat',
      location: { external: { bus: 'USB' } },
      capacity: Math.round(2000 * GB),
      free: Math.round(640 * GB),
      rotational: true,
    },
    {
      ...base,
      id: '/Volumes/Media',
      name: 'Media',
      mount: '/Volumes/Media',
      fs: 'smbfs',
      location: { network: { protocol: 'smb', host: 'nas.local' } },
      capacity: Math.round(7800 * GB),
      free: Math.round(2100 * GB),
      wholeVolume: false,
    },
  ];
}

interface Spec {
  name: string;
  /** Size in GB of a leaf; a folder's size is its children plus `loose`. */
  gb?: number;
  kind?: NodeKind;
  children?: Spec[];
  /** Small files directly inside, in GB. */
  loose?: number;
}

const leaf = (name: string, gb: number): Spec => ({ name, gb });
const dir = (name: string, children: Spec[], loose = 0): Spec => ({ name, children, loose });

/** What is on each mock volume. */
function specFor(volumeId: string): Spec {
  if (volumeId === '/Volumes/Photos') {
    return dir('Photos', [dir('2024', [leaf('wedding.mov', 48), leaf('trip-raw', 212)], 4), dir('2025', [leaf('studio.mov', 96), leaf('raw', 320)], 6), leaf('archive.zip', 41)], 2);
  }
  if (volumeId === '/Volumes/Media') {
    return dir('Media', [dir('Films', [leaf('film-a.mkv', 38), leaf('film-b.mkv', 31), leaf('film-c.mkv', 24)], 3), dir('Music', [leaf('library', 120)], 1), dir('Backups', [leaf('laptop.sparsebundle', 402)])]);
  }
  return dir(
    'Macintosh HD',
    [
      dir('Users', [
        dir('alex', [
          dir('Library', [dir('Developer', [dir('Xcode', [leaf('DerivedData', 41.2), leaf('iOS DeviceSupport', 12.5)], 0.4), leaf('CoreSimulator', 33.1)], 1), dir('Caches', [leaf('com.example.editor', 9.8)], 14.2), { name: 'Mail', kind: 'unreadable' as const }], 6.1),
          dir('Movies', [leaf('screen-recording.mov', 18.4), leaf('conference.mp4', 7.7)], 2.2),
          dir('Documents', [leaf('archive-2019.zip', 5.2)], 11.8),
          dir('Downloads', [leaf('installer.dmg', 4.1)], 6.6),
        ], 8.4),
      ]),
      dir('Applications', [leaf('Xcode.app', 38.9), leaf('Editor.app', 2.8)], 21.7),
      dir('Library', [leaf('Application Support', 22.4)], 5.3),
      dir('private', [leaf('var', 14.6)], 1.2),
      dir('opt', [], 4.8),
      { name: 'Volumes', kind: 'mountPoint' as const },
    ],
    9.3,
  );
}

interface Built {
  parent: number;
  name: string;
  kind: NodeKind;
  bytes: number;
  items: number;
  loose: number;
  newest: number;
  kids: number[];
}

/** A size tree built from a spec; ids follow the order of the spec. */
export class MockTree {
  private readonly nodes: Built[] = [];

  constructor(spec: Spec, private readonly newest: number) {
    this.build(spec, -1);
  }

  private build(spec: Spec, parent: number): number {
    const id = this.nodes.length;
    const kind = spec.kind ?? (spec.children ? 'folder' : 'file');
    const node: Built = { parent, name: spec.name, kind, bytes: 0, items: 0, loose: Math.round((spec.loose ?? 0) * GB), newest: this.newest - id * 3_600_000, kids: [] };
    this.nodes.push(node);
    for (const c of spec.children ?? []) {
      const child = this.build(c, id);
      node.kids.push(child);
      const k = this.nodes[child]!;
      node.bytes += k.bytes;
      node.items += k.items + 1;
    }
    if (!spec.children) node.bytes = Math.round((spec.gb ?? 0) * GB);
    node.bytes += node.loose;
    node.items += node.loose > 0 ? Math.max(1, Math.round(node.loose / (40 * MB))) : 0;
    return id;
  }

  get totalBytes(): number {
    return this.nodes[0]!.bytes;
  }

  private info(id: number): DiskNode {
    const n = this.nodes[id]!;
    return { id, name: n.name, kind: n.kind, bytes: n.bytes, items: n.items, newestChange: n.newest, complete: n.kind !== 'unreadable', hasChildren: n.kids.length > 0 || n.loose > 0 };
  }

  children(q: Pick<DiskChildrenQuery, 'node' | 'offset' | 'limit' | 'minFraction'>): DiskChildren | null {
    const id = q.node ?? 0;
    const parent = this.nodes[id];
    if (!parent) return null;
    const kids = [...parent.kids].sort((a, b) => this.nodes[b]!.bytes - this.nodes[a]!.bytes || this.nodes[a]!.name.localeCompare(this.nodes[b]!.name));
    const floor = q.minFraction > 0 ? Math.max(1, Math.ceil(parent.bytes * Math.min(q.minFraction, 1))) : 0;
    const shown = kids.filter((k) => this.nodes[k]!.bytes >= floor);
    const rest = kids.slice(shown.length);
    const other: Folded = { count: rest.length, bytes: rest.reduce((a, k) => a + this.nodes[k]!.bytes, 0), items: rest.reduce((a, k) => a + this.nodes[k]!.items, 0) };
    const limit = Math.min(Math.max(q.limit, 1), 256);
    const start = Math.min(q.offset, shown.length);
    const end = Math.min(start + limit, shown.length);
    const loose: Folded = { count: parent.loose > 0 ? 1 : 0, bytes: parent.loose, items: parent.loose > 0 ? 1 : 0 };
    return { node: this.info(id), children: shown.slice(start, end).map((k) => this.info(k)), other, loose, shown: shown.length, offset: start, nextOffset: end < shown.length ? end : null };
  }

  path(id: number, root: string): string | null {
    if (!this.nodes[id]) return null;
    const names: string[] = [];
    for (let at = id; at > 0; at = this.nodes[at]!.parent) names.unshift(this.nodes[at]!.name);
    return [root.replace(/\/$/, ''), ...names].join('/');
  }
}

interface Scan {
  status: DiskScanStatus;
  tree: MockTree;
  root: string;
  timers: Array<ReturnType<typeof setTimeout>>;
}

/** How a scan is paced: it reports at these shares of the total, the last one is the end. */
const STEPS = [0.12, 0.35, 0.6, 0.85, 1];

/**
 * The mock's scan jobs. Progress is scheduled with `later` (a timer in the browser build, a microtask when the mock has no
 * latency), so a test advances a scan without sleeping.
 */
export class MockDiskMap {
  private readonly scans = new Map<number, Scan>();
  private readonly listeners = new Set<(s: DiskScanStatus) => void>();
  private nextScan = 1;

  constructor(
    private readonly now: () => number,
    private readonly later: (fn: () => void, step: number) => ReturnType<typeof setTimeout>,
    private readonly error: (code: CommandError['code'], message: string) => CommandError,
  ) {}

  onStatus(listener: (s: DiskScanStatus) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(scan: Scan, patch: Partial<DiskScanStatus>): void {
    scan.status = { ...scan.status, ...patch, seq: scan.status.seq + 1 };
    const copy = structuredClone(scan.status);
    for (const l of this.listeners) l(copy);
  }

  volumes(): Volume[] {
    return mockVolumes();
  }

  start(target: DiskTarget): DiskScanStatus {
    const volumes = mockVolumes();
    const volume = 'volume' in target ? volumes.find((v) => v.id === target.volume.id) : undefined;
    if ('volume' in target && !volume) throw this.error('notFound', 'That volume is not mounted any more.');
    const running = [...this.scans.values()].find((s) => s.status.state === 'running' && JSON.stringify(s.status.target) === JSON.stringify(target));
    if (running) return structuredClone(running.status);
    if ([...this.scans.values()].filter((s) => s.status.state === 'running').length >= 2) throw this.error('busy', 'Two scans are already running. Stop one or wait for it to finish.');
    if ('folder' in target && !/^([a-zA-Z]:)?[\\/]/.test(target.folder.path)) throw this.error('badRequest', 'Give the full path of a folder.');
    const id = volume?.id ?? '/';
    const tree = new MockTree(specFor(id), this.now());
    const name = volume?.name ?? (target as { folder: { path: string } }).folder.path.split(/[\\/]/).filter(Boolean).pop() ?? '/';
    const total = volume && volume.wholeVolume && volume.capacity !== null && volume.free !== null ? volume.capacity - volume.free : null;
    const scan: Scan = {
      tree,
      root: volume?.mount ?? (target as { folder: { path: string } }).folder.path,
      timers: [],
      status: {
        scan: this.nextScan++,
        seq: 0,
        name,
        target,
        volume: volume ?? null,
        state: 'running',
        phase: 'readingVolumeFacts',
        progress: { bytes: 0, items: 0, folders: 0, totalBytes: total, unreadable: 0 },
        truncated: false,
        unreadableFirst: [],
      },
    };
    this.scans.set(scan.status.scan, scan);
    this.emit(scan, {});
    STEPS.forEach((share, i) => {
      scan.timers.push(this.later(() => this.step(scan, share, i === STEPS.length - 1), i + 1));
    });
    return structuredClone(scan.status);
  }

  private step(scan: Scan, share: number, last: boolean): void {
    if (scan.status.state !== 'running') return;
    const counted = Math.round(scan.tree.totalBytes * share);
    const where = scan.status.volume?.location;
    const shareDropped = typeof where === 'object' && 'network' in where && share >= 0.6;
    let state: ScanState = 'running';
    if (shareDropped) state = { failed: { failure: { volumeUnreachable: { detail: 'nas.local stopped answering' } } } };
    else if (last) state = 'done';
    const prev = scan.status.progress;
    this.emit(scan, {
      phase: 'readingFolders',
      state,
      progress: {
        ...prev,
        bytes: Math.max(prev.bytes, counted),
        items: Math.max(prev.items, Math.round(counted / (4 * MB))),
        folders: Math.max(prev.folders, Math.round(share * 1800)),
        totalBytes: prev.totalBytes === null ? null : Math.max(prev.totalBytes, counted),
        unreadable: share >= 0.35 ? 1 : 0,
      },
      unreadableFirst: share >= 0.35 ? ['/Users/alex/Library/Mail'] : [],
    });
  }

  cancel(scan: number): void {
    const s = this.find(scan);
    if (s.status.state !== 'running') return;
    s.timers.forEach(clearTimeout);
    this.emit(s, { state: 'cancelled' });
  }

  list(): DiskScanStatus[] {
    return [...this.scans.values()].map((s) => structuredClone(s.status));
  }

  children(q: DiskChildrenQuery): DiskChildren {
    if (!(q.minFraction >= 0 && q.minFraction <= 1)) throw this.error('badRequest', 'The smallest share shown must be between 0 and 1.');
    const page = this.find(q.scan).tree.children(q);
    if (!page) throw this.error('notFound', 'That folder is not in this scan.');
    return page;
  }

  path(scan: number, node: number): string {
    const p = this.find(scan).tree.path(node, this.find(scan).root);
    if (p === null) throw this.error('notFound', 'That folder is not in this scan.');
    return p;
  }

  private find(scan: number): Scan {
    const s = this.scans.get(scan);
    if (!s) throw this.error('notFound', 'That scan is no longer kept. Scan again.');
    return s;
  }
}

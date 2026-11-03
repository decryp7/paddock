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
  UnreadableReason,
  DiskRings,
  DiskRingsQuery,
  RingNode,
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
    { ...base, id: '/', name: 'Macintosh HD', mount: '/', fs: 'apfs', location: 'internal', capacity: Math.round(953 * GB), free: Math.round(210 * GB) },
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
          dir('Projects', [leaf('monorepo', 96), leaf('build output', 58), leaf('node_modules', 14)], 5),
        ], 8.4),
      ]),
      dir('Applications', [leaf('Xcode.app', 38.9), leaf('Editor.app', 2.8)], 21.7),
      dir('Library', [leaf('Application Support', 22.4)], 5.3),
      dir('private', [leaf('var', 14.6)], 1.2),
      dir('opt', [], 4.8),
      { name: 'Volumes', kind: 'mountPoint' as const },
    ],
    0,
  );
}

/** Small files take whole blocks, so their content is a little less than what they occupy. */
const looseLogical = (bytes: number): number => Math.round(bytes * 0.88);

interface Built {
  parent: number;
  name: string;
  kind: NodeKind;
  bytes: number;
  logical: number;
  items: number;
  loose: number;
  newest: number;
  kids: number[];
}

/** A size tree built from a spec; ids follow the order of the spec. */
/** The mock's only refused folder is ~/Library/Mail, which the engine on macOS reports as behind Full Disk Access. */
const MOCK_UNREADABLE_REASON: UnreadableReason = 'needsFullDiskAccess';

export class MockTree {
  private readonly nodes: Built[] = [];

  constructor(spec: Spec, private readonly newest: number) {
    this.build(spec, -1);
  }

  private build(spec: Spec, parent: number): number {
    const id = this.nodes.length;
    const kind = spec.kind ?? (spec.children ? 'folder' : 'file');
    const node: Built = { parent, name: spec.name, kind, bytes: 0, logical: 0, items: 0, loose: Math.round((spec.loose ?? 0) * GB), newest: this.newest - id * 3_600_000, kids: [] };
    this.nodes.push(node);
    for (const c of spec.children ?? []) {
      const child = this.build(c, id);
      node.kids.push(child);
      const k = this.nodes[child]!;
      node.bytes += k.bytes;
      node.logical += k.logical;
      node.items += k.items + 1;
    }
    if (!spec.children) {
      node.bytes = Math.round((spec.gb ?? 0) * GB);
      node.logical = node.bytes;
    }
    node.bytes += node.loose;
    node.logical += looseLogical(node.loose);
    node.items += node.loose > 0 ? Math.max(1, Math.round(node.loose / (40 * MB))) : 0;
    return id;
  }

  get totalBytes(): number {
    return this.nodes[0]!.bytes;
  }

  private info(id: number): DiskNode {
    const n = this.nodes[id]!;
    return { id, name: n.name, kind: n.kind, bytes: n.bytes, logicalBytes: n.logical, items: n.items, newestChange: n.newest, complete: n.kind !== 'unreadable', hasChildren: n.kids.length > 0 || n.loose > 0, unreadableReason: n.kind === 'unreadable' ? MOCK_UNREADABLE_REASON : null };
  }

  children(q: Pick<DiskChildrenQuery, 'node' | 'offset' | 'limit' | 'minFraction'>): DiskChildren | null {
    const id = q.node ?? 0;
    const parent = this.nodes[id];
    if (!parent) return null;
    const kids = [...parent.kids].sort((a, b) => this.nodes[b]!.bytes - this.nodes[a]!.bytes || this.nodes[a]!.name.localeCompare(this.nodes[b]!.name));
    const floor = q.minFraction > 0 ? Math.max(1, Math.ceil(parent.bytes * Math.min(q.minFraction, 1))) : 0;
    const shown = kids.filter((k) => this.nodes[k]!.bytes >= floor);
    const rest = kids.slice(shown.length);
    const other: Folded = { count: rest.length, bytes: rest.reduce((a, k) => a + this.nodes[k]!.bytes, 0), logicalBytes: rest.reduce((a, k) => a + this.nodes[k]!.logical, 0), items: rest.reduce((a, k) => a + this.nodes[k]!.items, 0) };
    const limit = Math.min(Math.max(q.limit, 1), 256);
    const start = Math.min(q.offset, shown.length);
    const end = Math.min(start + limit, shown.length);
    const loose: Folded = { count: parent.loose > 0 ? 1 : 0, bytes: parent.loose, logicalBytes: looseLogical(parent.loose), items: parent.loose > 0 ? 1 : 0 };
    return { node: this.info(id), children: shown.slice(start, end).map((k) => this.info(k)), other, loose, shown: shown.length, offset: start, nextOffset: end < shown.length ? end : null };
  }

  /** The levels under `id` nested, like the engine's `rings`: breadth first, biggest first, folded by share of the centre. */
  rings(id: number, rings: number, minShare: number, maxNodes: number): DiskRings | null {
    const centre = this.nodes[id];
    if (!centre) return null;
    const floor = minShare > 0 ? Math.max(1, Math.ceil(centre.bytes * Math.min(minShare, 1))) : 0;
    const made = (nodeId: number): RingNode => {
      const n = this.nodes[nodeId]!;
      return { node: this.info(nodeId), children: [], other: { count: 0, bytes: 0, logicalBytes: 0, items: 0 }, loose: { count: n.loose > 0 ? 1 : 0, bytes: n.loose, logicalBytes: looseLogical(n.loose), items: n.loose > 0 ? 1 : 0 } };
    };
    const root = made(id);
    let count = 1;
    let capped = false;
    const queue: Array<{ out: RingNode; id: number; depth: number }> = [{ out: root, id, depth: 0 }];
    for (let head = 0; head < queue.length; head++) {
      const { out, id: at, depth } = queue[head]!;
      if (depth >= rings) continue;
      const kids = [...this.nodes[at]!.kids].sort((a, b) => this.nodes[b]!.bytes - this.nodes[a]!.bytes || this.nodes[a]!.name.localeCompare(this.nodes[b]!.name));
      for (const k of kids) {
        const kid = this.nodes[k]!;
        if (kid.bytes < floor || count >= maxNodes) {
          if (kid.bytes >= floor) capped = true;
          out.other = { count: out.other.count + 1, bytes: out.other.bytes + kid.bytes, logicalBytes: out.other.logicalBytes + kid.logical, items: out.other.items + kid.items };
        } else {
          const child = made(k);
          out.children.push(child);
          count++;
          queue.push({ out: child, id: k, depth: depth + 1 });
        }
      }
    }
    return { root, count, capped };
  }

  path(id: number, root: string): string | null {
    if (!this.nodes[id]) return null;
    const names: string[] = [];
    for (let at = id; at > 0; at = this.nodes[at]!.parent) names.unshift(this.nodes[at]!.name);
    // The root's path is the folder the scan started from, never empty.
    return names.length === 0 ? root : [root.replace(/\/$/, ''), ...names].join('/');
  }
}

const scaleNode = (n: DiskNode, f: number): DiskNode => ({ ...n, bytes: Math.round(n.bytes * f), logicalBytes: Math.round(n.logicalBytes * f), items: Math.round(n.items * f) });
const scaleFolded = (n: Folded, f: number): Folded => ({ ...n, bytes: Math.round(n.bytes * f), logicalBytes: Math.round(n.logicalBytes * f), items: Math.round(n.items * f) });

/** A level as it looks when only the share `f` of the tree has been counted. */
function scalePage(page: DiskChildren, f: number): DiskChildren {
  return { ...page, node: scaleNode(page.node, f), children: page.children.map((c) => scaleNode(c, f)), other: scaleFolded(page.other, f), loose: scaleFolded(page.loose, f) };
}

function scaleRing(r: RingNode, f: number): RingNode {
  return { node: scaleNode(r.node, f), children: r.children.map((c) => scaleRing(c, f)), other: scaleFolded(r.other, f), loose: scaleFolded(r.loose, f) };
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
  /** The paths "revealed", for tests. */
  readonly revealed: string[] = [];

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

  /**
   * A saved map, as the agent offers one after a restart (ADR 0029): a finished scan of `target` that was written to the cache
   * `ageMs` ago. `changedBytes` is the net change in used space since then; `null` for a folder.
   */
  seedSaved(target: DiskTarget, ageMs: number, changedBytes: number | null): DiskScanStatus {
    const started = this.start(target, null);
    const scan = this.find(started.scan);
    scan.timers.forEach(clearTimeout);
    scan.timers = [];
    const total = scan.tree.totalBytes;
    const at = this.now() - ageMs;
    this.emit(scan, {
      state: 'done',
      phase: 'readingFolders',
      progress: { ...scan.status.progress, bytes: total, items: Math.round(total / (4 * MB)), folders: 1800, totalBytes: scan.status.progress.totalBytes === null ? null : Math.max(total, scan.status.progress.totalBytes) },
      scannedAt: at,
      origin: { saved: { savedAt: at } },
      changedBytes,
    });
    return structuredClone(scan.status);
  }

  /** Forgets one saved map (a scan of this run stays). */
  forgetSaved(scan: number): void {
    const s = this.find(scan);
    if (typeof s.status.origin === 'object') this.scans.delete(scan);
  }

  /** Forgets every saved map. */
  forgetAllSaved(): void {
    for (const [id, s] of this.scans) if (typeof s.status.origin === 'object') this.scans.delete(id);
  }

  start(target: DiskTarget, replaces: number | null = null): DiskScanStatus {
    const volumes = mockVolumes();
    const volume = 'volume' in target ? volumes.find((v) => v.id === target.volume.id) : undefined;
    if ('volume' in target && !volume) throw this.error('notFound', 'That volume is not mounted any more.');
    const running = [...this.scans.values()].find((s) => s.status.state === 'running' && JSON.stringify(s.status.target) === JSON.stringify(target));
    if (running) return structuredClone(running.status);
    if ([...this.scans.values()].filter((s) => s.status.state === 'running').length >= 2) throw this.error('busy', 'There is not room for another scan next to the running ones. Finish or close a scan first.');
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
        needsFullDiskAccess: false,
        itemsPerSecond: null,
        etaSeconds: null,
        hiddenBytes: null,
        scannedAt: null,
        origin: 'live',
        changedBytes: null,
        replaces: replaces !== null && this.scans.get(replaces) && JSON.stringify(this.scans.get(replaces)?.status.target) === JSON.stringify(target) ? replaces : null,
        notSaved: null,
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
      needsFullDiskAccess: share >= 0.35 && scan.status.volume?.id === '/',
      itemsPerSecond: state === 'running' ? Math.round(counted / (4 * MB) / (share * 10)) : null,
      etaSeconds: state === 'running' && prev.totalBytes !== null ? Math.round((1 - share) * 40) : null,
      hiddenBytes: state === 'done' && prev.totalBytes !== null ? Math.max(0, prev.totalBytes - Math.max(prev.bytes, counted)) : null,
      scannedAt: state === 'running' ? scan.status.scannedAt : this.now(),
    });
  }

  cancel(scan: number): void {
    const s = this.find(scan);
    if (s.status.state !== 'running') return;
    s.timers.forEach(clearTimeout);
    // Stopping keeps what the first pass had counted, so the partial result is never empty.
    const floor = Math.round(s.tree.totalBytes * STEPS[0]!);
    this.emit(s, { state: 'cancelled', scannedAt: this.now(), progress: { ...s.status.progress, bytes: Math.max(s.status.progress.bytes, floor) } });
  }

  /** Reads the folders that could not be opened again, and only those; the mock's Mail folder stays closed (no access). */
  retry(scan: number): DiskScanStatus {
    const s = this.find(scan);
    if (s.status.state === 'running') throw this.error('busy', 'That scan is still running.');
    if (s.status.state !== 'done' || s.status.progress.unreadable === 0) throw this.error('badRequest', 'There is nothing to read again.');
    this.emit(s, { state: 'running', phase: 'readingFolders', progress: { ...s.status.progress, unreadable: 0 }, unreadableFirst: [] });
    const status = structuredClone(s.status);
    s.timers.push(
      this.later(() => {
        if (s.status.state !== 'running') return;
        this.emit(s, {
          state: 'done',
          scannedAt: this.now(),
          progress: { ...s.status.progress, folders: s.status.progress.folders + 1, unreadable: 1 },
          unreadableFirst: ['/Users/alex/Library/Mail'],
        });
      }, 1),
    );
    return status;
  }

  /** Shows a node in the file manager: the mock only checks that the node exists and remembers it. */
  reveal(scan: number, node: number): void {
    this.revealed.push(this.path(scan, node));
  }

  /** The kept scans, except those a finished Rescan has taken the place of. */
  list(): DiskScanStatus[] {
    const all = [...this.scans.values()];
    const replaced = new Set(all.filter((s) => s.status.state === 'done' && s.status.replaces !== null).map((s) => s.status.replaces));
    return all.filter((s) => !replaced.has(s.status.scan)).map((s) => structuredClone(s.status));
  }

  children(q: DiskChildrenQuery): DiskChildren {
    if (!(q.minFraction >= 0 && q.minFraction <= 1)) throw this.error('badRequest', 'The smallest share shown must be between 0 and 1.');
    const scan = this.find(q.scan);
    const page = scan.tree.children(q);
    if (!page) throw this.error('notFound', 'That folder is not in this scan.');
    // The levels grow with what has been counted, so a scan that is not finished never shows more than its progress says.
    const f = Math.min(1, scan.status.progress.bytes / Math.max(1, scan.tree.totalBytes));
    return f >= 1 ? page : scalePage(page, f);
  }

  rings(q: DiskRingsQuery): DiskRings {
    if (!(q.rings >= 1 && q.rings <= 6)) throw this.error('badRequest', 'Ask for 1 to 6 rings.');
    if (!(q.minShare >= 0 && q.minShare <= 1)) throw this.error('badRequest', 'The smallest share shown must be between 0 and 1.');
    if (!(q.maxNodes >= 1 && q.maxNodes <= 2000)) throw this.error('badRequest', 'Ask for 1 to 2000 nodes.');
    const out = this.find(q.scan).tree.rings(q.node ?? 0, q.rings, q.minShare, q.maxNodes);
    if (!out) throw this.error('notFound', 'That folder is not in this scan.');
    const scan = this.find(q.scan);
    const f = Math.min(1, scan.status.progress.bytes / Math.max(1, scan.tree.totalBytes));
    return f >= 1 ? out : { ...out, root: scaleRing(out.root, f) };
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

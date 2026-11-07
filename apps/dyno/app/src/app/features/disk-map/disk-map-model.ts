import type { PdSunburstNode } from '@decryp7/paddock-ui/chart';
import type { DiskChildren, DiskNode, RingNode, DiskScanStatus, ScanFailure, ScanProgress, ScanState, Volume } from '../../core/api.types';
import { formatStorage, formatCount, formatDuration, plural } from '../../shared/format';
import type { WindowPlatform } from '../../core/platform';
import { volumeAgeNote } from '../../shared/volume-state';

/**
 * Presentation helpers of the disk map. The engine owns every size, count and state; this file only turns what it sent into
 * words, widths and groups (rule: no domain logic in the UI).
 */

export type VolumeGroup = 'internal' | 'external' | 'network';

export const VOLUME_GROUPS: { id: VolumeGroup; label: string; hint: string | null }[] = [
  { id: 'internal', label: 'Internal', hint: null },
  { id: 'external', label: 'External', hint: null },
  { id: 'network', label: 'Network', hint: 'Slower to scan; a dropped link stops the scan.' },
];

export function volumeGroup(v: Volume): VolumeGroup {
  if (v.location === 'internal') return 'internal';
  return 'external' in v.location ? 'external' : 'network';
}

const NETWORK_NAME: Record<string, string> = { smb: 'SMB', nfs: 'NFS', afp: 'AFP', webDav: 'WebDAV', sshfs: 'sshfs', other: 'Network' };

/** "APFS · USB", "SMB network volume": the file system and where the volume lives. */
export function volumeMeta(v: Volume): string {
  const fs = v.fs.toUpperCase();
  if (v.location === 'internal') return `${fs} · internal`;
  if ('external' in v.location) return v.location.external.bus ? `${fs} · ${v.location.external.bus}` : `${fs} · external`;
  return `${NETWORK_NAME[v.location.network.protocol] ?? 'Network'} network volume`;
}

/**
 * "Free space not reported" or "378.2 GB free" (a share's is the server's: "on the server"); figures that are not current say
 * so: "378.2 GB free · As of 14:05", "Not answering since 14:05".
 */
export function volumeFree(v: Volume): string {
  if (!v.reachable) return volumeAgeNote(v) ?? 'Not answering';
  if (v.free === null) return 'Free space not reported';
  const free = v.wholeVolume ? `${formatStorage(v.free)} free` : `${formatStorage(v.free)} free on the server`;
  const note = volumeAgeNote(v);
  return note ? `${free} · ${note}` : free;
}

/** The share (0 to 100) of `part` in `whole`; 0 when the whole is empty. */
export function sharePercent(part: number, whole: number): number {
  return whole > 0 && Number.isFinite(part) ? Math.min(100, (part / whole) * 100) : 0;
}

export function shareText(part: number, whole: number): string {
  const p = sharePercent(part, whole);
  if (p === 0) return '0%';
  return p < 1 ? '<1%' : `${Math.round(p)}%`;
}

export function phaseLabel(s: DiskScanStatus): string {
  return s.phase === 'readingVolumeFacts' ? 'Reading volume facts' : 'Reading folders';
}

/** "1,116,387 items · 325.9 GB of 568.6 GB counted", or just the growing count when the total is unknown. */
export function jobCount(p: ScanProgress): string {
  const items = plural(p.items, 'item');
  return p.totalBytes === null ? `${items} · ${formatStorage(p.bytes)} counted` : `${items} · ${formatStorage(p.bytes)} of ${formatStorage(p.totalBytes)} counted`;
}

/** 0 to 1 for a determinate bar, `null` when the total is unknown (a growing count instead). */
export function jobFraction(p: ScanProgress): number | null {
  return p.totalBytes === null || p.totalBytes <= 0 ? null : Math.min(1, p.bytes / p.totalBytes);
}

export type ScanOutcome = 'running' | 'done' | 'cancelled' | 'failed';

export function outcome(state: ScanState): ScanOutcome {
  return typeof state === 'string' ? state : 'failed';
}

export interface FailureText {
  kind: 'network' | 'start' | 'internal';
  title: string;
  detail: string;
}

/** What a failed scan says: what stopped, what is kept and what Retry does. */
export function failureText(f: ScanFailure, name: string): FailureText {
  if ('volumeUnreachable' in f) {
    return {
      kind: 'network',
      title: `Scan stopped: ${f.volumeUnreachable.detail}`,
      detail: `The link to ${name} dropped. What was counted is kept, and the folders it did not finish are marked incomplete. Retry scans ${name} again.`,
    };
  }
  if ('cannotStart' in f) return { kind: 'start', title: `Dyno can't scan ${name}`, detail: f.cannotStart.reason };
  return { kind: 'internal', title: 'The scan stopped unexpectedly', detail: f.internal.detail };
}

/** "Stopped by you at 56%": how far a stopped scan got, when the total is known. */
export function stoppedAt(p: ScanProgress): string {
  const f = jobFraction(p);
  return f === null ? `Stopped by you after ${formatStorage(p.bytes)}` : `Stopped by you at ${Math.round(f * 100)}%`;
}

/** The whole "Dyno can't read it" state: the engine says Full Disk Access is the likely reason and nothing at all was counted. */
export function nothingReadable(s: DiskScanStatus | null): boolean {
  return !!s && s.needsFullDiskAccess && outcome(s.state) !== 'running' && s.progress.bytes === 0;
}

/** "1,850 items/s" and "about 40 s left", only when the engine knows them (never a made-up number). */
export function jobRate(s: DiskScanStatus): string | null {
  const parts: string[] = [];
  if (s.itemsPerSecond !== null) parts.push(`${formatCount(s.itemsPerSecond)} items/s`);
  if (s.etaSeconds !== null) parts.push(`about ${formatDuration(s.etaSeconds)} left`);
  return parts.length ? parts.join(' · ') : null;
}

/** The label of the "show it in the file manager" action on this system. */
export function revealLabel(platform: WindowPlatform): string {
  return platform === 'macos' ? 'Reveal in Finder' : platform === 'windows' ? 'Show in Explorer' : 'Show in folder';
}

/** The capacity strip: used, free and how the used part divides. All figures come from the volume and the scan. */
export interface CapacityStrip {
  /** Bytes, or null when the volume cannot say. */
  capacity: number | null;
  used: number | null;
  free: number | null;
  counted: number;
  /** Used space the walk has not counted yet (shrinks while it runs). */
  notCounted: number;
  /** The engine's one "Hidden space" figure; 0 when it sends none. */
  hidden: number;
  /** Counted more than used (clones count once per copy). */
  overCounted: boolean;
  /** Widths in percent of the capacity. */
  width: { counted: number; notCounted: number; hidden: number; free: number };
}

/** `hiddenBytes` is the engine's figure (null when it sends none); the page shows it as is. */
export function capacityStrip(volume: Volume | null, progress: ScanProgress, state: ScanState, hiddenBytes: number | null = null): CapacityStrip | null {
  if (!volume || volume.capacity === null || volume.capacity <= 0) return null;
  const capacity = volume.capacity;
  const free = volume.free;
  const used = free === null ? null : Math.max(0, capacity - free);
  const finished = outcome(state) === 'done';
  const counted = progress.bytes;
  const rest = used === null ? 0 : Math.max(0, used - counted);
  const hidden = hiddenBytes ?? 0;
  const notCounted = finished && volume.wholeVolume ? 0 : rest;
  const w = (n: number) => sharePercent(n, capacity);
  return {
    capacity,
    used,
    free,
    counted,
    notCounted,
    hidden,
    overCounted: used !== null && counted > used,
    width: { counted: w(Math.min(counted, used ?? counted)), notCounted: w(notCounted), hidden: w(hidden), free: free === null ? 0 : w(free) },
  };
}

export interface Crumb {
  node: number | null;
  name: string;
  /** The folder's size when it was last read (the chart needs it to wrap the centre). */
  bytes: number;
}

/** A child's row in the list and the table. */
export interface LevelRow {
  node: DiskNode;
  /** Position among the children by size (0 is the largest): the ring colours the first five by it, the list swatch follows. */
  rank: number;
  percent: number;
  share: string;
  drillable: boolean;
}

export function levelRows(level: DiskChildren): LevelRow[] {
  return level.children.map((node, i) => ({
    node,
    rank: i,
    percent: sharePercent(node.bytes, level.node.bytes),
    share: shareText(node.bytes, level.node.bytes),
    drillable: node.hasChildren && node.kind === 'folder',
  }));
}

export type SortKey = 'name' | 'bytes' | 'items' | 'newestChange';

/** Sorts rows for the table (a presentation order of what the engine sent). */
export function sortRows(rows: LevelRow[], key: SortKey, dir: 'asc' | 'desc'): LevelRow[] {
  const sign = dir === 'asc' ? 1 : -1;
  return [...rows].sort((a, b) => {
    const d = key === 'name' ? a.node.name.localeCompare(b.node.name) : a.node[key] - b.node[key];
    return d === 0 ? a.node.id - b.node.id : sign * d;
  });
}

export function filterRows(rows: LevelRow[], text: string): LevelRow[] {
  const q = text.trim().toLocaleLowerCase();
  return q ? rows.filter((r) => r.node.name.toLocaleLowerCase().includes(q)) : rows;
}

/** The node shape of the shared ring chart (`pd-sunburst`). */
export type ChartNode = PdSunburstNode;

export const chartId = (node: number): string => `n${node}`;

/** The node number behind a chart id (`n12`), or null for hidden space, the ghost and the aggregates. */
export function nodeOfChartId(id: string): number | null {
  const m = /^n(\d+)$/.exec(id);
  return m ? Number(m[1]) : null;
}

/** What is not in the folders at the volume root: hidden space (done) or the not-yet-counted ghost (running or stopped). */
export interface RootExtras {
  hidden: number;
  pending: number;
}

function ringNode(r: RingNode): ChartNode {
  const c = r.node;
  const id = chartId(c.id);
  const kids: ChartNode[] = r.children.map(ringNode);
  if (r.other.count > 0 && r.other.bytes > 0) {
    kids.push({ id: `${id}-other`, label: plural(r.other.count, 'smaller item'), value: r.other.bytes, kind: 'other', detail: plural(r.other.items, 'item') });
  }
  if (r.loose.bytes > 0) {
    kids.push({ id: `${id}-loose`, label: 'Files directly in this folder', value: r.loose.bytes, kind: 'other', detail: plural(r.loose.items, 'item') });
  }
  return {
    id,
    label: c.name,
    value: c.bytes,
    children: kids,
    // A folder at the edge of the answer has more inside that was not sent: opening it asks the engine.
    hasMore: r.children.length === 0 && c.kind === 'folder' && c.hasChildren,
    kind: 'item',
    detail: `${plural(c.items, 'item')}${c.complete ? '' : ', incomplete'}`,
  };
}

/** An enclosing folder the chart needs only to go up: its size and the one folder on the way down. */
export interface Ancestor {
  id: number;
  name: string;
  bytes: number;
}

/**
 * The chart's tree for one answer of the engine's rings query. At the scan's root, hidden space and the ghost sit beside the
 * folders and count in the root's value; deeper down the enclosing folders wrap the centre so the chart can go back up.
 */
export function toChartTree(rings: RingNode, ancestors: readonly Ancestor[] = [], extras?: RootExtras): ChartNode {
  let node = ringNode(rings);
  const extra = ancestors.length === 0 && extras ? extras : null;
  if (extra && (extra.hidden > 0 || extra.pending > 0)) {
    const kids = [...(node.children ?? [])];
    if (extra.hidden > 0) kids.push({ id: 'hidden', label: 'Hidden space', value: extra.hidden, kind: 'hidden', detail: 'Space the folders do not account for' });
    if (extra.pending > 0) kids.push({ id: 'pending', label: 'Not counted yet', value: extra.pending, kind: 'pending' });
    node = { ...node, value: node.value + extra.hidden + extra.pending, children: kids };
  }
  for (let i = ancestors.length - 1; i >= 0; i--) {
    const a = ancestors[i]!;
    node = { id: chartId(a.id), label: a.name, value: Math.max(a.bytes, node.value), children: [node], kind: 'item' };
  }
  return node;
}

/** The folders to open, in order, to get from the centre to the node `to` inside a rings answer; null when it is not there. */
export function chainTo(from: RingNode, to: number): DiskNode[] | null {
  for (const c of from.children) {
    if (c.node.id === to) return [c.node];
    const deeper = chainTo(c, to);
    if (deeper) return [c.node, ...deeper];
  }
  return null;
}

/** Finds a node anywhere in a rings answer (for the chart's menu). */
export function findRingNode(from: RingNode, id: number): DiskNode | null {
  if (from.node.id === id) return from.node;
  for (const c of from.children) {
    const hit = findRingNode(c, id);
    if (hit) return hit;
  }
  return null;
}

/** What a saved map says about itself: its age is the page's "scanned" line; this is the rest. `null` for a map of this run. */
export function savedNote(s: DiskScanStatus, ago: string, bytes: (n: number) => string): string | null {
  if (typeof s.origin === 'string') return null;
  const change =
    s.changedBytes === null
      ? ''
      : s.changedBytes === 0
        ? ' Used space is the same as when it was scanned (a net figure: files deleted and written add up).'
        : ` Net change in used space since: ${s.changedBytes > 0 ? '+' : '\u2212'}${bytes(Math.abs(s.changedBytes))} (a net figure: files deleted and written add up).`;
  return `Saved map, scanned ${ago}.${change} Rescan to see what changed.`;
}

/** Why a finished scan was not kept for the next time, in words; `null` when it was kept. */
export function notSavedText(s: DiskScanStatus): string | null {
  switch (s.notSaved) {
    case 'tooBig':
      return 'Too big to keep; raise the limit in Settings.';
    case 'noSpace':
      return 'Not kept for next time: the disk would be left with too little free space.';
    case 'failed':
      return 'Not kept for next time: writing the map failed. The earlier saved map is unchanged.';
    default:
      return null;
  }
}

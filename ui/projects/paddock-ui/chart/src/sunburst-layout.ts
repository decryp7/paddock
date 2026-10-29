/**
 * The pure core of `pd-sunburst` (docs/plans/pd-sunburst.md section 4): layout of a value tree into annular sectors, the sector
 * path, branch colours, label fit and the tree keyboard model. No Angular, no DOM; odd input (NaN, negative, empty, huge) gives a
 * defined result and never throws.
 */
import { ringOrder } from './ring';

/** What a node is: `hidden` is hatched and not drillable, `pending` a ghost ("not counted yet"), `other` a consumer aggregate. */
export type PdSunburstKind = 'item' | 'hidden' | 'pending' | 'other';

/** One node of the hierarchy. `value` is the node's total including its children. */
export interface PdSunburstNode {
  /** Stable across updates. */
  readonly id: string;
  readonly label: string;
  /** At least 0; NaN, negative and non-finite values count as 0. */
  readonly value: number;
  readonly children?: readonly PdSunburstNode[];
  /** Children exist but are not loaded: drilling emits `loadChildren`. */
  readonly hasMore?: boolean;
  readonly kind?: PdSunburstKind;
  /** One line for the centre and the accessible name ("297,155 items"). */
  readonly detail?: string;
}

/** A node under the pointer or focus. */
export interface PdSunburstPoint {
  readonly node: PdSunburstNode;
  /** Ids from the first ring's ancestor down to the node (the centre is not included). */
  readonly path: readonly string[];
  /** The node's value over its parent's, 0..1. */
  readonly share: number;
}

/** One drawn sector. Angles are degrees, clockwise from 12 o'clock. */
export interface PdArc {
  /** The node id, or `<parent>\u0000folded` for the "n smaller items" sector. */
  readonly id: string;
  /** Id of the parent sector; null in the first ring. */
  readonly parent: string | null;
  /** Id of the parent node (the centre's id in the first ring). */
  readonly parentNodeId: string;
  readonly parentLabel: string;
  readonly parentValue: number;
  /** 1 is the ring next to the centre. */
  readonly depth: number;
  readonly a0: number;
  readonly a1: number;
  readonly kind: PdSunburstKind | 'folded';
  /** Palette slot (0-based), or -1 for the neutral colour. */
  readonly slot: number;
  /** 1 on every second sibling: a little darker, so neighbours of one branch stay apart. */
  readonly tone: 0 | 1;
  readonly label: string;
  readonly value: number;
  /** The node; a synthetic `other` node for a folded sector. */
  readonly node: PdSunburstNode;
  /** Ids folded into a "smaller items" sector. */
  readonly foldedIds: readonly string[];
  /** Whether its children are drawn in the next ring. */
  readonly open: boolean;
  /** Whether it has children (drawn or not) or more to load. */
  readonly expandable: boolean;
  readonly posinset: number;
  readonly setsize: number;
}

/** How the colour of a node is chosen: by branch, or a palette slot (or 'neutral') by identity. */
export type PdSunburstColourBy = 'branch' | ((node: PdSunburstNode) => number | 'neutral');

export interface PdSunburstLayoutOptions {
  /** Rings drawn below the centre, 1 to 6 (default 4). */
  readonly rings?: number;
  /** Degrees below which a sector folds into "n smaller items" (default 1.5, minimum 1). */
  readonly minAngle?: number;
  /** Children per parent before folding (default 48). */
  readonly maxChildren?: number;
  /** Sectors before `minAngle` rises (default 1000, hard 2000). */
  readonly maxSegments?: number;
  /** Fixed denominator, so sectors only grow while a total fills; null: the centre's value. */
  readonly scale?: number | null;
  readonly colourBy?: PdSunburstColourBy;
  /** Palette slots for the five largest branches, from {@link branchSlots}. */
  readonly order?: readonly number[];
}

export interface PdSunburstLayout {
  readonly arcs: readonly PdArc[];
  /** Set when the segment cap raised the minimum angle. */
  readonly folded: { readonly segments: number; readonly minAngle: number } | null;
  /** The denominator used. */
  readonly scale: number;
  /** The node in the middle (the root for an unknown id). */
  readonly centre: PdSunburstNode;
  /** The root down to the centre. */
  readonly trail: readonly PdSunburstNode[];
}

/** The hard cap on drawn sectors. */
export const SUNBURST_MAX_SEGMENTS = 2000;
/** The branches that get their own colour. */
export const SUNBURST_BRANCH_SLOTS = 5;
const KIND_RANK: Readonly<Record<PdSunburstKind, number>> = { item: 0, other: 1, hidden: 2, pending: 3 };

/** A value as a size: finite and not negative, else 0. */
export function sizeOf(value: number): number {
  return Number.isFinite(value) && value > 0 ? value : 0;
}

/** Clamps `n` to `[lo, hi]`; NaN and non-numbers give `fallback`. */
export function clampOr(n: number, lo: number, hi: number, fallback: number): number {
  return Number.isFinite(n) ? Math.min(hi, Math.max(lo, n)) : fallback;
}

/** The palette slots (ring order, colour-vision safe) of the five largest branches. */
export function branchSlots(palette: readonly string[]): readonly number[] {
  return ringOrder(palette, SUNBURST_BRANCH_SLOTS);
}

/** The ids from the root to `id`, as nodes; `[root]` when `id` is null, the root or unknown. */
export function findTrail(root: PdSunburstNode, id: string | null): readonly PdSunburstNode[] {
  if (id === null || id === root.id) return [root];
  const parents = new Map<PdSunburstNode, PdSunburstNode>();
  const stack: PdSunburstNode[] = [root];
  while (stack.length) {
    const n = stack.pop() as PdSunburstNode;
    for (const c of n.children ?? []) {
      parents.set(c, n);
      if (c.id === id) {
        const trail: PdSunburstNode[] = [];
        for (let at: PdSunburstNode | undefined = c; at; at = parents.get(at)) trail.push(at);
        return trail.reverse();
      }
      stack.push(c);
    }
  }
  return [root];
}

const byValueThenId = (a: { v: number; node: PdSunburstNode }, b: { v: number; node: PdSunburstNode }): number =>
  b.v - a.v || (a.node.id < b.node.id ? -1 : a.node.id > b.node.id ? 1 : 0);

function plural(n: number): string {
  return `${n.toLocaleString('en-US')} smaller item${n === 1 ? '' : 's'}`;
}

interface Pass {
  readonly arcs: PdArc[];
  readonly rings: number;
  readonly minAngle: number;
  readonly maxChildren: number;
  readonly degPerUnit: number;
  readonly colourBy: PdSunburstColourBy;
  readonly order: readonly number[];
}

/** Places the children of `node` (effective size `size`, starting at `a0`) and recurses while rings remain. */
function expand(p: Pass, node: PdSunburstNode, parentArc: PdArc | null, size: number, a0: number, depth: number, slotOfParent: number): void {
  const kids = node.children ?? [];
  if (depth > p.rings || kids.length === 0 || size <= 0) return;
  const sum = kids.reduce((s, k) => s + sizeOf(k.value), 0);
  const factor = sum > size ? size / sum : 1;
  const items: { v: number; node: PdSunburstNode }[] = [];
  const rest: { v: number; node: PdSunburstNode }[] = [];
  for (const k of kids) {
    const entry = { v: sizeOf(k.value) * factor, node: k };
    (!k.kind || k.kind === 'item' ? items : rest).push(entry);
  }
  items.sort(byValueThenId);
  rest.sort((a, b) => KIND_RANK[a.node.kind ?? 'item'] - KIND_RANK[b.node.kind ?? 'item'] || byValueThenId(a, b));
  const keep = items.filter((e, i) => i < p.maxChildren && e.v * p.degPerUnit >= p.minAngle);
  const tail = items.slice(keep.length);
  const foldedValue = tail.reduce((s, e) => s + e.v, 0);

  type Slice = { v: number; node: PdSunburstNode; folded: readonly string[] };
  const slices: Slice[] = keep.map((e) => ({ ...e, folded: [] }));
  if (tail.length > 0) {
    const id = `${node.id}\u0000folded`;
    slices.push({ v: foldedValue, node: { id, label: plural(tail.length), value: foldedValue, kind: 'other' }, folded: tail.map((e) => e.node.id) });
  }
  for (const e of rest) slices.push({ ...e, folded: [] });
  const drawn = slices.filter((s) => s.v > 0);

  let at = a0;
  let rank = 0;
  drawn.forEach((s, i) => {
    const kind: PdArc['kind'] = s.folded.length ? 'folded' : (s.node.kind ?? 'item');
    const a1 = at + s.v * p.degPerUnit;
    let slot = -1;
    if (kind === 'item') {
      if (typeof p.colourBy === 'function') {
        const c = p.colourBy(s.node);
        slot = c === 'neutral' || !Number.isFinite(c) ? -1 : Math.max(0, Math.floor(c));
      } else if (depth === 1) {
        slot = p.order[rank] ?? -1;
        rank += 1;
      } else slot = slotOfParent;
    }
    const arc: PdArc = {
      id: s.node.id,
      parent: parentArc?.id ?? null,
      parentNodeId: node.id,
      parentLabel: node.label,
      parentValue: size,
      depth,
      a0: at,
      a1,
      kind,
      slot,
      tone: depth >= 2 && i % 2 === 1 ? 1 : 0,
      label: s.node.label,
      value: s.v,
      node: s.node,
      foldedIds: s.folded,
      open: false,
      expandable: (s.node.children?.length ?? 0) > 0 || s.node.hasMore === true,
      posinset: i + 1,
      setsize: drawn.length,
    };
    p.arcs.push(arc);
    if (kind === 'item' && depth < p.rings && (s.node.children?.length ?? 0) > 0) {
      const before = p.arcs.length;
      expand(p, s.node, arc, s.v, at, depth + 1, slot);
      if (p.arcs.length > before) p.arcs[before - 1] = { ...arc, open: true };
    }
    at = a1;
  });
}

/**
 * Lays `root` out around the node `centreId`. Sizes add up per parent; children larger than their parent are scaled to fit; sectors
 * under `minAngle` (and children beyond `maxChildren`) fold into one "n smaller items" sector per parent; when more than
 * `maxSegments` sectors would be drawn `minAngle` rises until it fits. Ties are ordered by id, so equal values lay out the same way.
 */
export function sunburstLayout(root: PdSunburstNode, centreId: string | null, opts: PdSunburstLayoutOptions = {}): PdSunburstLayout {
  const trail = findTrail(root, centreId);
  const centre = trail[trail.length - 1] as PdSunburstNode;
  const rings = Math.round(clampOr(opts.rings ?? 4, 1, 6, 4));
  const requested = clampOr(opts.minAngle ?? 1.5, 1, 360, 1.5);
  const maxChildren = Math.round(clampOr(opts.maxChildren ?? 48, 1, 100000, 48));
  const maxSegments = Math.round(clampOr(opts.maxSegments ?? 1000, 1, SUNBURST_MAX_SEGMENTS, 1000));
  const centreValue = sizeOf(centre.value);
  const fixed = opts.scale === null || opts.scale === undefined ? 0 : sizeOf(opts.scale);
  const scale = Math.max(fixed, centreValue);
  if (scale <= 0) return { arcs: [], folded: null, scale: 0, centre, trail };
  const degPerUnit = 360 / scale;
  let minAngle = requested;
  let arcs: PdArc[] = [];
  for (let i = 0; i < 24; i += 1) {
    arcs = [];
    expand({ arcs, rings, minAngle, maxChildren, degPerUnit, colourBy: opts.colourBy ?? 'branch', order: opts.order ?? [0, 1, 2, 3, 4] }, centre, null, centreValue, 0, 1, -1);
    if (arcs.length <= maxSegments || minAngle >= 360) break;
    minAngle = Math.min(360, minAngle * 1.5);
  }
  return { arcs, folded: minAngle > requested ? { segments: arcs.length, minAngle } : null, scale, centre, trail };
}

/** Inner and outer radius of each ring: ring widths shrink outwards (58/50/40/34 px for four rings between 92 and 280). */
export function ringRadii(rings: number, inner: number, outer: number): readonly { r0: number; r1: number }[] {
  const n = Math.round(clampOr(rings, 1, 6, 4));
  const weights = Array.from({ length: n }, (_, i) => Math.max(0.3, 1 - 0.14 * i));
  const total = weights.reduce((s, w) => s + w, 0);
  const span = Math.max(0, outer - inner);
  let r = inner;
  return weights.map((w) => {
    const r0 = r;
    r += (w / total) * span;
    return { r0, r1: r };
  });
}

const RAD = Math.PI / 180;
const f2 = (n: number): string => (Math.round(n * 100) / 100).toString();
const pt = (cx: number, cy: number, r: number, deg: number): string => `${f2(cx + r * Math.sin(deg * RAD))} ${f2(cy - r * Math.cos(deg * RAD))}`;

/**
 * The path of an annular sector with a constant pixel gap at every radius (`gapPx`, 1.8 by default; negative grows it, for a focus
 * ring). A full ring is two circles (drawn even-odd). Sectors too thin for the gap are drawn without it.
 */
export function arcPath(cx: number, cy: number, r0: number, r1: number, a0: number, a1: number, gapPx = 1.8): string {
  const half = gapPx / 2;
  let ro = r1 - half;
  let ri = r0 + half;
  if (ro <= ri) {
    ro = r1;
    ri = r0;
  }
  ri = Math.max(0.5, ri);
  if (a1 - a0 >= 359.99) {
    const circle = (r: number): string => `M ${f2(cx + r)} ${f2(cy)} A ${f2(r)} ${f2(r)} 0 1 0 ${f2(cx - r)} ${f2(cy)} A ${f2(r)} ${f2(r)} 0 1 0 ${f2(cx + r)} ${f2(cy)} Z`;
    return `${circle(ro)} ${circle(ri)}`;
  }
  const inset = (r: number): [number, number] => {
    const d = (half / r) / RAD;
    const s = a0 + d;
    const e = a1 - d;
    if (e > s) return [s, e];
    const mid = (a0 + a1) / 2;
    return [mid - 0.05, mid + 0.05];
  };
  const [so, eo] = inset(ro);
  const [si, ei] = inset(ri);
  const large = (a: number, b: number): number => (b - a > 180 ? 1 : 0);
  return `M ${pt(cx, cy, ro, so)} A ${f2(ro)} ${f2(ro)} 0 ${large(so, eo)} 1 ${pt(cx, cy, ro, eo)} L ${pt(cx, cy, ri, ei)} A ${f2(ri)} ${f2(ri)} 0 ${large(si, ei)} 0 ${pt(cx, cy, ri, si)} Z`;
}

/** The estimated width of `text` in a 12 px system face. */
export function textWidth(text: string): number {
  return text.length * 6.6;
}

/** Whether a curved label fits: the arc length at the ring's middle must hold the text plus 18 px, and the ring must be 16 px thick. */
export function labelFits(arcLength: number, thickness: number, text: string): boolean {
  return thickness >= 16 && arcLength >= textWidth(text) + 18;
}

/** The path a label follows, at radius `r`; reversed on the lower half so the text reads upright. */
export function labelPath(cx: number, cy: number, r: number, a0: number, a1: number): string {
  const mid = (((a0 + a1) / 2) % 360 + 360) % 360;
  const flip = mid > 90 && mid < 270;
  const [s, e] = flip ? [a1, a0] : [a0, a1];
  return `M ${pt(cx, cy, r, s)} A ${f2(r)} ${f2(r)} 0 ${Math.abs(a1 - a0) > 180 ? 1 : 0} ${flip ? 0 : 1} ${pt(cx, cy, r, e)}`;
}

/** WCAG relative luminance of 0..255 channels. */
export function luminance(r: number, g: number, b: number): number {
  const lin = (c: number): number => {
    const x = c / 255;
    return x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/** WCAG contrast ratio between two luminances. */
export function contrastRatio(l1: number, l2: number): number {
  const [hi, lo] = l1 > l2 ? [l1, l2] : [l2, l1];
  return (hi + 0.05) / (lo + 0.05);
}

/**
 * The tree keyboard model: Up and Down move among siblings only and stop at the ends; Right goes to the first child and Left to the
 * parent (the only keys that change level); Home and End jump among siblings; a printable character jumps to the next sibling whose
 * label starts with it. Returns the id to focus, or null when nothing moves.
 */
export function nextFocus(arcs: readonly PdArc[], current: string, key: string): string | null {
  const cur = arcs.find((a) => a.id === current);
  if (!cur) return null;
  const siblings = arcs.filter((a) => a.parent === cur.parent && a.depth === cur.depth);
  const at = siblings.indexOf(cur);
  let target: PdArc | undefined;
  switch (key) {
    case 'ArrowUp':
      target = siblings[at - 1];
      break;
    case 'ArrowDown':
      target = siblings[at + 1];
      break;
    case 'Home':
      target = siblings[0];
      break;
    case 'End':
      target = siblings[siblings.length - 1];
      break;
    case 'ArrowRight':
      target = arcs.find((a) => a.parent === cur.id);
      break;
    case 'ArrowLeft':
      target = cur.parent === null ? undefined : arcs.find((a) => a.id === cur.parent);
      break;
    default:
      if ([...key].length === 1 && key.trim() !== '') {
        const c = key.toLowerCase();
        const ordered = [...siblings.slice(at + 1), ...siblings.slice(0, at)];
        target = ordered.find((a) => a.label.toLowerCase().startsWith(c));
      }
  }
  return target && target.id !== current ? target.id : null;
}

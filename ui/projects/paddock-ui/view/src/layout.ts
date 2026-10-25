import type { ActionRef, Tone } from '../generated/view';
import { crossNearArrow, flattenPath, inflate, pathCrosses, pointAt, roundedPath, type Pt, type Rect } from './route';

/** A box of a drawn graph or diagram. */
export interface SceneNode {
  readonly id: string;
  readonly label: string;
  /** Extra lines under the label (a diagram element's detail, one per line). */
  readonly detail: readonly string[];
  /** A small caption above the label (a diagram element's group). */
  readonly caption: string;
  readonly tone: Tone;
  readonly action: ActionRef | null;
  /** The detail lines are the members of a class: a rule under the name and left-aligned lines, not centred text. */
  readonly compartment: boolean;
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

/** A line between two boxes. `path` is SVG path data; the label sits at (`lx`, `ly`). */
export interface SceneEdge {
  readonly from: string;
  readonly to: string;
  readonly label: string;
  readonly path: string;
  readonly lx: number;
  readonly ly: number;
  /** How the label sits at (`lx`, `ly`): `middle` over a horizontal line, `start` beside a line that runs mostly up and down. */
  readonly anchor: 'start' | 'middle';
}

/** A sequence diagram's vertical line under a participant. */
export interface Lifeline {
  readonly x: number;
  readonly y1: number;
  readonly y2: number;
}

/** What to draw: positions are final, in SVG user units. */
export interface Scene {
  readonly kind: 'layered' | 'radial' | 'sequence';
  readonly width: number;
  readonly height: number;
  readonly nodes: readonly SceneNode[];
  readonly edges: readonly SceneEdge[];
  readonly lifelines: readonly Lifeline[];
}

/** What the layout takes for one box. */
export interface LayoutNode {
  readonly id: string;
  readonly label: string;
  readonly detail?: string;
  readonly caption?: string;
  readonly tone?: Tone;
  readonly action?: ActionRef | null;
  readonly compartment?: boolean;
}
export interface LayoutEdge {
  readonly from: string;
  readonly to: string;
  readonly label: string;
}

const PAD = 16;
const LAYER_GAP = 56;
const NODE_GAP = 28;
const LINE_H = 16;
const CHAR_W = 7;
const MIN_W = 96;
const MAX_W = 240;
const ROW_H = 44;
const COL_W = 168;
/** The room a status glyph takes at the start of a box, in user units. */
export const MARK_W = 20;
const RING_GAP = 24;

/** Splits a detail into its lines (bounded: at most 12 lines of 60 characters are drawn). */
export function detailLines(detail: string | undefined): readonly string[] {
  if (!detail) return [];
  return detail
    .split(/\r?\n/)
    .filter((l) => l.trim() !== '')
    .slice(0, 12)
    .map((l) => (l.length > 60 ? `${l.slice(0, 59)}…` : l));
}

function sized(n: LayoutNode): Pick<SceneNode, 'w' | 'h' | 'detail'> {
  const detail = detailLines(n.detail);
  const longest = Math.max(n.label.length, n.caption?.length ?? 0, ...detail.map((l) => l.length));
  // A box with a status carries its mark (a glyph) before the label, so it is wider by the mark.
  const mark = n.tone && n.tone !== 'neutral' ? MARK_W : 0;
  const w = Math.min(MAX_W, Math.max(MIN_W, Math.ceil(longest * CHAR_W + 28 + mark)));
  // A class: the name above a rule, then the members with the same room (8 units) above the first and under the last.
  const h = n.compartment && detail.length ? (n.caption ? 38 : 30) + 12 + detail.length * LINE_H : 36 + (n.caption ? 12 : 0) + detail.length * LINE_H + (detail.length ? 6 : 0);
  return { w, h, detail };
}

/** The layer of each node: the longest path from a source, ignoring the edges that close a cycle (found by a depth-first walk). Deterministic. */
export function layers(count: number, edges: readonly (readonly [number, number])[]): number[] {
  const out: number[][] = Array.from({ length: count }, () => []);
  for (const [a, b] of edges) if (a !== b) out[a]?.push(b);
  const color = new Uint8Array(count);
  const back = new Set<string>();
  for (let s = 0; s < count; s++) {
    if (color[s]) continue;
    const stack: [number, number][] = [[s, 0]];
    color[s] = 1;
    while (stack.length) {
      const top = stack[stack.length - 1] as [number, number];
      const next = out[top[0]]?.[top[1]++];
      if (next === undefined) {
        color[top[0]] = 2;
        stack.pop();
      } else if (color[next] === 1) back.add(`${top[0]}>${next}`);
      else if (!color[next]) {
        color[next] = 1;
        stack.push([next, 0]);
      }
    }
  }
  const indeg = new Array<number>(count).fill(0);
  const dag: number[][] = Array.from({ length: count }, () => []);
  for (let a = 0; a < count; a++) {
    for (const b of out[a] ?? []) {
      if (back.has(`${a}>${b}`)) continue;
      dag[a]?.push(b);
      indeg[b] = (indeg[b] ?? 0) + 1;
    }
  }
  const layer = new Array<number>(count).fill(0);
  const queue: number[] = [];
  indeg.forEach((d, i) => d === 0 && queue.push(i));
  for (let q = 0; q < queue.length; q++) {
    const a = queue[q] as number;
    for (const b of dag[a] ?? []) {
      layer[b] = Math.max(layer[b] ?? 0, (layer[a] ?? 0) + 1);
      indeg[b] = (indeg[b] ?? 1) - 1;
      if (indeg[b] === 0) queue.push(b);
    }
  }
  return layer;
}

/** Orders the nodes inside each layer by the mean place of their neighbours in the layer before (two sweeps), keeping input order for ties. */
function ordered(rows: number[][], edges: readonly (readonly [number, number])[], layer: readonly number[]): void {
  const pos = new Map<number, number>();
  const incoming = new Map<number, number[]>();
  for (const [a, b] of edges) if (layer[a] === (layer[b] ?? 0) - 1) (incoming.get(b) ?? incoming.set(b, []).get(b))?.push(a);
  const place = (): void => rows.forEach((r) => r.forEach((n, i) => pos.set(n, i)));
  place();
  for (let sweep = 0; sweep < 2; sweep++) {
    for (let l = 1; l < rows.length; l++) {
      const key = new Map<number, number>();
      for (const n of rows[l] ?? []) {
        const ps = (incoming.get(n) ?? []).map((a) => pos.get(a) ?? 0);
        key.set(n, ps.length ? ps.reduce((x, y) => x + y, 0) / ps.length : (pos.get(n) ?? 0));
      }
      (rows[l] as number[]).sort((a, b) => (key.get(a) ?? 0) - (key.get(b) ?? 0) || (pos.get(a) ?? 0) - (pos.get(b) ?? 0));
      place();
    }
  }
}

function boxEdge(n: { x: number; y: number; w: number; h: number }, tx: number, ty: number): [number, number] {
  const cx = n.x + n.w / 2;
  const cy = n.y + n.h / 2;
  const dx = tx - cx;
  const dy = ty - cy;
  if (dx === 0 && dy === 0) return [cx, cy];
  const s = Math.min(dx === 0 ? Infinity : n.w / 2 / Math.abs(dx), dy === 0 ? Infinity : n.h / 2 / Math.abs(dy));
  return [cx + dx * s, cy + dy * s];
}

/** The rectangle an edge label covers (12-unit text, 7 units a character, the baseline at `ly`): what the placer keeps clear of boxes and lines. */
export function labelRect(lx: number, ly: number, anchor: 'start' | 'middle', label: string): Rect {
  const w = label.length * CHAR_W;
  return { x: anchor === 'middle' ? lx - w / 2 : lx, y: ly - 11, w, h: 14 };
}

const r1 = (n: number): number => Math.round(n * 10) / 10;

type Box = Rect;

const overlap = (a: Rect, b: Rect): boolean => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

/**
 * A route from one box to another along an arc outside every box of a radial drawing (centred on the middle of the boxes), going the
 * short way round first and the long way second; the short way when neither is clear. Steps of 30 degrees with rounded corners read as an arc.
 */
function arcAround(boxes: readonly Box[], s: Box, t: Box, blockers: readonly Box[], lane: number): { path: string; mid: Pt; dir: Pt } {
  const minX = Math.min(...boxes.map((b) => b.x));
  const maxX = Math.max(...boxes.map((b) => b.x + b.w));
  const minY = Math.min(...boxes.map((b) => b.y));
  const maxY = Math.max(...boxes.map((b) => b.y + b.h));
  const c: Pt = [(minX + maxX) / 2, (minY + maxY) / 2];
  const rOut = Math.hypot((maxX - minX) / 2, (maxY - minY) / 2) + 18 + (lane % 3) * 10;
  const angle = (b: Box): number => Math.atan2(b.y + b.h / 2 - c[1], b.x + b.w / 2 - c[0]);
  const [as, at] = [angle(s), angle(t)];
  const short = Math.atan2(Math.sin(at - as), Math.cos(at - as));
  const long = short > 0 ? short - 2 * Math.PI : short + 2 * Math.PI;
  let shortWay: { path: string; mid: Pt; dir: Pt } | null = null;
  for (const sweep of [short, long]) {
    const steps = Math.max(2, Math.ceil(Math.abs(sweep) / (Math.PI / 6)));
    const via: Pt[] = Array.from({ length: steps + 1 }, (_, i) => [c[0] + rOut * Math.cos(as + (sweep * i) / steps), c[1] + rOut * Math.sin(as + (sweep * i) / steps)]);
    const [x1, y1] = boxEdge(s, via[0]![0], via[0]![1]);
    const [x2, y2] = boxEdge(t, via[steps]![0], via[steps]![1]);
    const path = roundedPath([[x1, y1], ...via, [x2, y2]], 24);
    const m = via[Math.floor(steps / 2)] as Pt;
    const route = { path, mid: m, dir: [Math.cos(as + sweep / 2 + Math.PI / 2) * Math.sign(sweep), Math.sin(as + sweep / 2 + Math.PI / 2) * Math.sign(sweep)] as Pt };
    shortWay ??= route;
    if (!pathCrosses(path, blockers)) return route;
  }
  return shortWay as { path: string; mid: Pt; dir: Pt };
}

/** The hub of a radial graph: the node with the most connections (either direction), the first one in the input on a tie. */
export function hubOf(count: number, pairs: readonly (readonly [number, number])[]): number {
  const degree = new Array<number>(count).fill(0);
  for (const [a, b] of pairs) {
    if (a === b) continue;
    degree[a] = (degree[a] ?? 0) + 1;
    degree[b] = (degree[b] ?? 0) + 1;
  }
  let best = 0;
  degree.forEach((d, i) => {
    if (d > (degree[best] ?? 0)) best = i;
  });
  return best;
}

/** Distance in edges from `hub` (edges taken in both directions); nodes it cannot reach (another component) are one ring beyond the farthest. */
export function ringsFrom(hub: number, count: number, pairs: readonly (readonly [number, number])[]): number[] {
  const near: number[][] = Array.from({ length: count }, () => []);
  for (const [a, b] of pairs) {
    if (a === b) continue;
    near[a]?.push(b);
    near[b]?.push(a);
  }
  const ring = new Array<number>(count).fill(-1);
  ring[hub] = 0;
  const queue = [hub];
  for (let q = 0; q < queue.length; q++) {
    const a = queue[q] as number;
    for (const b of near[a] ?? []) {
      if (ring[b] === -1) {
        ring[b] = (ring[a] ?? 0) + 1;
        queue.push(b);
      }
    }
  }
  const far = Math.max(0, ...ring) + 1;
  return ring.map((r) => (r === -1 ? far : r));
}

/**
 * Where a radial layout puts each box (centres) and the size of the canvas that fits them. The best-connected node is the hub in the
 * middle; the others sit on rings by their distance from it (another component is one ring beyond), each ring wide enough that its
 * boxes cannot touch (a box is treated as its circumscribed circle) and each ring far enough from the one inside it. Inside a ring
 * the nodes follow the mean direction of their neighbours on the ring before. The canvas is the real bounding box of the boxes plus
 * the padding, so a small graph is a small drawing.
 */
function radialPlace(
  boxes: readonly { w: number; h: number }[],
  pairs: readonly (readonly [number, number])[],
): { at: ({ x: number; y: number } | undefined)[]; shiftX: number; shiftY: number; width: number; height: number } {
  const count = boxes.length;
  if (count === 0) return { at: [], shiftX: 0, shiftY: 0, width: MIN_W + PAD * 2, height: PAD * 2 + 36 };
  const hub = hubOf(count, pairs);
  const ring = ringsFrom(hub, count, pairs);
  const rings: number[][] = [];
  ring.forEach((r, i) => (rings[r] ??= []).push(i));
  const radius = (i: number): number => Math.hypot(boxes[i]?.w ?? 0, boxes[i]?.h ?? 0) / 2;
  const at: ({ x: number; y: number } | undefined)[] = new Array(count);
  const angle = new Array<number>(count).fill(0);
  at[hub] = { x: 0, y: 0 };
  let inner = 0;
  let innerRadius = radius(hub);
  for (let k = 1; k < rings.length; k++) {
    const members = rings[k] ?? [];
    if (members.length === 0) continue;
    const reach = Math.max(...members.map(radius));
    // Far enough from the ring inside (centres on two circles are at least their radii apart), and wide enough for its own boxes.
    const around = members.length > 1 ? (2 * reach + RING_GAP) / (2 * Math.sin(Math.PI / members.length)) : 0;
    const r = Math.max(inner + innerRadius + reach + RING_GAP, around);
    // Neighbours on the ring inside decide the order: the mean direction of the ones already placed.
    const key = new Map<number, number>();
    for (const n of members) {
      const sx = pairs.reduce((sum, [a, b]) => sum + (a === n && ring[b] === k - 1 ? Math.cos(angle[b] ?? 0) : b === n && ring[a] === k - 1 ? Math.cos(angle[a] ?? 0) : 0), 0);
      const sy = pairs.reduce((sum, [a, b]) => sum + (a === n && ring[b] === k - 1 ? Math.sin(angle[b] ?? 0) : b === n && ring[a] === k - 1 ? Math.sin(angle[a] ?? 0) : 0), 0);
      key.set(n, sx === 0 && sy === 0 ? Infinity : Math.atan2(sy, sx));
    }
    members.sort((a, b) => (key.get(a) ?? Infinity) - (key.get(b) ?? Infinity) || a - b);
    members.forEach((n, i) => {
      const a = (2 * Math.PI * i) / members.length - Math.PI / 2;
      angle[n] = a;
      at[n] = { x: r * Math.cos(a), y: r * Math.sin(a) };
    });
    inner = r;
    innerRadius = reach;
  }
  let [minX, minY, maxX, maxY] = [Infinity, Infinity, -Infinity, -Infinity];
  at.forEach((p, i) => {
    if (!p) return;
    const w = boxes[i]?.w ?? 0;
    const h = boxes[i]?.h ?? 0;
    minX = Math.min(minX, p.x - w / 2);
    maxX = Math.max(maxX, p.x + w / 2);
    minY = Math.min(minY, p.y - h / 2);
    maxY = Math.max(maxY, p.y + h / 2);
  });
  return { at, shiftX: PAD - minX, shiftY: PAD - minY, width: maxX - minX + PAD * 2, height: maxY - minY + PAD * 2 };
}

/**
 * A layered (top-down) layout of a graph or diagram: nodes in layers by longest path from a source (cycles are broken by a depth-first
 * walk), ordered inside a layer to cut crossings, each layer centred; edges are curves from the bottom of the source to the top of the
 * target, labels at their middle. `radial` puts the layers on rings around the first layer instead. Pure and deterministic; the
 * `force` layout of the wire type is drawn as `layered`, because a force simulation is not reproducible.
 */
export function layoutGraph(nodes: readonly LayoutNode[], edges: readonly LayoutEdge[], radial = false): Scene {
  const first = layoutGraphAt(nodes, edges, radial, 0);
  if (!radial) return first;
  // A line that had to bow around a box can swing past the left or top of the canvas: draw again with room for it.
  let low = 0;
  for (const edge of first.edges) {
    for (const p of flattenPath(edge.path)) low = Math.min(low, p[0] - 4, p[1] - 4);
    // A label may sit past the left or top as well.
    const r = edge.label ? labelRect(edge.lx, edge.ly, edge.anchor, edge.label) : null;
    if (r) low = Math.min(low, r.x - 4, r.y - 4);
  }
  return low < 0 ? layoutGraphAt(nodes, edges, radial, Math.ceil(-low)) : first;
}

/**
 * Puts each edge label where its rectangle touches no box, no edge (its own included) and no label placed before it. The first try is
 * the position the router gave (beside the middle of the line); a bowed edge's label is tried first at the apex of the bow, on the
 * outward side (away from the middle of the drawing), half the label's extent plus 4 units from the line. If that touches something
 * the placer walks, in this order: distance (0, 8, 16 units further out), then the place along the line (middle, then 40/60, 30/70,
 * 20/80 percent), then the side (outward, then the other). The first clear rectangle wins; with none, the one with the fewest hits.
 * A radial label always moves to its apex candidate when that is clear; a layered one keeps the router's position when that is clear.
 */
function placeLabels(edges: readonly SceneEdge[], boxes: readonly Rect[], radial: boolean): SceneEdge[] {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const b of boxes) {
    minX = Math.min(minX, b.x);
    minY = Math.min(minY, b.y);
    maxX = Math.max(maxX, b.x + b.w);
    maxY = Math.max(maxY, b.y + b.h);
  }
  const centre: Pt = [(minX + maxX) / 2, (minY + maxY) / 2];
  const taken: Rect[] = [];
  return edges.map((edge) => {
    if (!edge.label) return edge;
    const pts = flattenPath(edge.path);
    const hits = (lx: number, ly: number, anchor: 'start' | 'middle'): number => {
      const r = inflate(labelRect(lx, ly, anchor, edge.label), 1);
      return boxes.filter((b) => overlap(r, b)).length + edges.filter((o) => pathCrosses(o.path, [r])).length + taken.filter((t) => overlap(r, t)).length;
    };
    let best = { lx: edge.lx, ly: edge.ly, anchor: edge.anchor, score: hits(edge.lx, edge.ly, edge.anchor) };
    if (best.score > 0 || radial) {
      const w = edge.label.length * CHAR_W;
      search: for (const extra of [0, 8, 16]) {
        for (const f of [0.5, 0.4, 0.6, 0.3, 0.7, 0.2, 0.8]) {
          const { at, dir } = pointAt(pts, f);
          const normal: Pt = [-dir[1], dir[0]];
          // Outward: away from the middle of the drawing for a radial one, to the right (then up) for a layered one.
          const out = radial ? normal[0] * (at[0] - centre[0]) + normal[1] * (at[1] - centre[1]) : normal[0] || -normal[1];
          for (const sign of out >= 0 ? [1, -1] : [-1, 1]) {
            const [nx, ny] = [normal[0] * sign, normal[1] * sign];
            const gap = (w / 2) * Math.abs(nx) + 7 * Math.abs(ny) + 4 + extra;
            const [lx, ly] = [at[0] + nx * gap, at[1] + ny * gap + 4];
            const score = hits(lx, ly, 'middle');
            if (score < best.score || (score === 0 && radial)) best = { lx, ly, anchor: 'middle', score };
            if (score === 0) break search;
          }
        }
      }
    }
    taken.push(labelRect(best.lx, best.ly, best.anchor, edge.label));
    return { ...edge, lx: r1(best.lx), ly: r1(best.ly), anchor: best.anchor };
  });
}

function layoutGraphAt(nodes: readonly LayoutNode[], edges: readonly LayoutEdge[], radial: boolean, extra: number): Scene {
  const index = new Map(nodes.map((n, i) => [n.id, i]));
  const pairs: [number, number][] = [];
  for (const e of edges) {
    const a = index.get(e.from);
    const b = index.get(e.to);
    if (a !== undefined && b !== undefined) pairs.push([a, b]);
  }
  const layer = layers(nodes.length, pairs);
  const rows: number[][] = [];
  layer.forEach((l, i) => (rows[l] ??= []).push(i));
  for (let l = 0; l < rows.length; l++) rows[l] ??= [];
  ordered(rows, pairs, layer);
  const dims = nodes.map(sized);
  const boxes: { x: number; y: number; w: number; h: number }[] = nodes.map((_, i) => ({ x: 0, y: 0, w: dims[i]?.w ?? MIN_W, h: dims[i]?.h ?? 36 }));
  let width = 0;
  let height: number;
  if (radial) {
    const placed = radialPlace(boxes, pairs);
    boxes.forEach((box, i) => {
      const p = placed.at[i];
      box.x = r1((p?.x ?? 0) - box.w / 2 + placed.shiftX + extra);
      box.y = r1((p?.y ?? 0) - box.h / 2 + placed.shiftY + extra);
    });
    width = placed.width + extra;
    height = placed.height + extra;
  } else {
    const widths = rows.map((row) => row.reduce((sum, n) => sum + (boxes[n]?.w ?? 0), 0) + Math.max(0, row.length - 1) * NODE_GAP);
    width = Math.max(MIN_W, ...widths) + PAD * 2;
    let y = PAD;
    rows.forEach((row, l) => {
      let x = PAD + (width - PAD * 2 - (widths[l] ?? 0)) / 2;
      let rowH = 0;
      for (const n of row) {
        const box = boxes[n] as { x: number; y: number; w: number; h: number };
        box.x = r1(x);
        box.y = y;
        x += box.w + NODE_GAP;
        rowH = Math.max(rowH, box.h);
      }
      y += rowH + LAYER_GAP;
    });
    height = Math.max(y - LAYER_GAP + PAD, PAD * 2 + 36);
  }
  const rowTop = rows.map((row) => Math.min(...row.map((n) => boxes[n]?.y ?? 0)));
  const rowBottom = rows.map((row) => Math.max(...row.map((n) => (boxes[n]?.y ?? 0) + (boxes[n]?.h ?? 0))));
  const sceneNodes: SceneNode[] = nodes.map((n, i) => {
    const b = boxes[i] as { x: number; y: number; w: number; h: number };
    return { id: n.id, label: n.label, detail: dims[i]?.detail ?? [], caption: n.caption ?? '', tone: n.tone ?? 'neutral', action: n.action ?? null, compartment: n.compartment === true && (dims[i]?.detail.length ?? 0) > 0, ...b };
  });
  let backs = 0;
  const routed: { path: string }[] = [];
  let reach = 0;
  let reachY = 0;
  const sceneEdges: SceneEdge[] = edges.flatMap((e) => {
    const a = index.get(e.from);
    const b = index.get(e.to);
    if (a === undefined || b === undefined) return [];
    const s = boxes[a] as { x: number; y: number; w: number; h: number };
    const t = boxes[b] as { x: number; y: number; w: number; h: number };
    const edge = (path: string, lx: number, ly: number, anchor: 'start' | 'middle'): SceneEdge => ({ from: e.from, to: e.to, label: e.label, path, lx: r1(lx), ly: r1(ly), anchor });
    if (a === b) {
      const x = s.x + s.w;
      const y = s.y + s.h / 2;
      reach = Math.max(reach, x + 44 + e.label.length * CHAR_W);
      return [edge(`M ${r1(x)} ${r1(y - 8)} C ${r1(x + 36)} ${r1(y - 24)}, ${r1(x + 36)} ${r1(y + 24)}, ${r1(x)} ${r1(y + 8)}`, x + 44, y + 4, 'start')];
    }
    const blockers = boxes.filter((b) => b !== s && b !== t).map((b) => inflate(b, 3));
    if (radial) {
      const sc: Pt = [s.x + s.w / 2, s.y + s.h / 2];
      const tc: Pt = [t.x + t.w / 2, t.y + t.h / 2];
      const [px, py] = [tc[0] - sc[0], tc[1] - sc[1]];
      // A straight line unless it would cross another box: then the line bows to one side, then the other, a little more each time.
      const bows = [0, 0.25, -0.25, 0.5, -0.5, 0.8, -0.8, 1.2, -1.2, 1.7, -1.7, 2.4, -2.4];
      // A bow that also crosses no earlier edge near its arrowhead is preferred; the first one that only clears the boxes is the fallback.
      const earlier = routed.map((r) => r.path);
      let chosen: { path: string; mid: Pt; dir: Pt } | null = null;
      let clear: { path: string; mid: Pt; dir: Pt } | null = null;
      for (const bow of bows) {
        const ctl: Pt = [(sc[0] + tc[0]) / 2 + -py * bow, (sc[1] + tc[1]) / 2 + px * bow];
        const [x1, y1] = boxEdge(s, ctl[0], ctl[1]);
        const [x2, y2] = boxEdge(t, ctl[0], ctl[1]);
        const path = bow === 0 ? `M ${r1(x1)} ${r1(y1)} L ${r1(x2)} ${r1(y2)}` : `M ${r1(x1)} ${r1(y1)} Q ${r1(ctl[0])} ${r1(ctl[1])} ${r1(x2)} ${r1(y2)}`;
        const mid: Pt = bow === 0 ? [(x1 + x2) / 2, (y1 + y2) / 2] : [(x1 + 2 * ctl[0] + x2) / 4, (y1 + 2 * ctl[1] + y2) / 4];
        if (pathCrosses(path, blockers)) continue;
        clear ??= { path, mid, dir: [x2 - x1, y2 - y1] };
        if (!earlier.some((o) => crossNearArrow(path, o))) {
          chosen = { path, mid, dir: [x2 - x1, y2 - y1] };
          break;
        }
      }
      chosen ??= clear;
      // No bow clears the boxes (a ring packed tight around the hub): go round the outside of everything, the short way, then the long way.
      if (!chosen) {
        chosen = arcAround(boxes, s, t, blockers, backs++);
      }
      const c = chosen as { path: string; mid: Pt; dir: Pt };
      routed.push(c);
      // The label starts beside the line, not on it: 10 units to the side that points up (placeLabels moves it where it touches anything).
      const len = Math.hypot(c.dir[0], c.dir[1]) || 1;
      const side = c.dir[0] >= 0 ? -1 : 1;
      return [edge(c.path, c.mid[0] + (side * -c.dir[1] * 10) / len, c.mid[1] + (side * c.dir[0] * 10) / len, 'middle')];
    }
    const down = (layer[b] ?? 0) > (layer[a] ?? 0);
    const x1 = s.x + s.w / 2;
    const y1 = s.y + s.h;
    const x2 = t.x + t.w / 2;
    const y2 = t.y;
    const ym = (y1 + y2) / 2;
    const direct = `M ${r1(x1)} ${r1(y1)} C ${r1(x1)} ${r1(ym)}, ${r1(x2)} ${r1(ym)}, ${r1(x2)} ${r1(y2)}`;
    if (down && !pathCrosses(direct, blockers)) {
      // Beside the middle of the curve, not on it.
      return [edge(direct, (x1 + x2) / 2 + 8, ym, 'start')];
    }
    // An edge that goes back up (or sideways), or one that would cross a box on the way down, runs outside every box in a lane on the
    // right and through the free space between the rows, so it never crosses a box.
    const k = backs++;
    const bx = Math.max(...boxes.map((x) => x.x + x.w)) + 24 + (k % 4) * 10;
    const below = (rowBottom[layer[a] ?? 0] ?? s.y + s.h) + 12 + (k % 3) * 4;
    const above = (rowTop[layer[b] ?? 0] ?? t.y) - 8 - (k % 3) * 3;
    const exits: Pt[][] = [[[s.x + s.w, s.y + s.h / 2], [bx, s.y + s.h / 2]], [[x1, s.y + s.h], [x1, below], [bx, below]]];
    const enters: Pt[][] = [[[bx, t.y + t.h / 2], [t.x + t.w, t.y + t.h / 2]], [[bx, above], [x2, above], [x2, t.y]]];
    const tries: [number, number][] = down ? [[1, 1]] : [[0, 0], [1, 0], [0, 1], [1, 1]];
    let route: Pt[] = [];
    for (const [ex, en] of tries) {
      route = [...(exits[ex] as Pt[]), ...(enters[en] as Pt[])];
      if (!pathCrosses(roundedPath(route), blockers)) break;
    }
    const outer = route.filter((p) => p[0] === bx);
    reach = Math.max(reach, bx + 8 + e.label.length * CHAR_W);
    reachY = Math.max(reachY, below + PAD / 2);
    return [edge(roundedPath(route), bx + 8, ((outer[0] as Pt)[1] + (outer[outer.length - 1] as Pt)[1]) / 2, 'start')];
  });
  const placed = placeLabels(sceneEdges, boxes, radial);
  // A line that bows around a box may swing past the right or bottom of the boxes, and a label may sit past them: the canvas holds them.
  for (const edge of placed) {
    for (const p of flattenPath(edge.path)) {
      reach = Math.max(reach, p[0] + 4 - PAD - 24);
      reachY = Math.max(reachY, p[1] + 4);
    }
    if (edge.label) {
      const r = labelRect(edge.lx, edge.ly, edge.anchor, edge.label);
      reach = Math.max(reach, r.x + r.w + 4 - PAD - 24);
      reachY = Math.max(reachY, r.y + r.h + 4);
    }
  }
  return { kind: radial ? 'radial' : 'layered', width: r1(Math.max(width, reach + PAD + 24)), height: r1(Math.max(height, reachY)), nodes: sceneNodes, edges: placed, lifelines: [] };
}

/**
 * A sequence diagram: the elements are the participants left to right with a lifeline each, the relations are the messages from top
 * to bottom in the order given (the plugin's order is the time order); a message to oneself is a small loop.
 */
export function layoutSequence(nodes: readonly LayoutNode[], edges: readonly LayoutEdge[]): Scene {
  const colX = (i: number): number => PAD + i * COL_W + COL_W / 2;
  const headH = 40;
  const sceneNodes: SceneNode[] = nodes.map((n, i) => {
    const d = sized(n);
    const w = Math.min(COL_W - 16, d.w);
    return { id: n.id, label: n.label, detail: [], caption: n.caption ?? '', tone: n.tone ?? 'neutral', action: n.action ?? null, compartment: false, x: r1(colX(i) - w / 2), y: PAD, w, h: headH };
  });
  const index = new Map(nodes.map((n, i) => [n.id, i]));
  const bottom = PAD + headH + 24 + edges.length * ROW_H + 8;
  const sceneEdges: SceneEdge[] = edges.flatMap((e, k) => {
    const a = index.get(e.from);
    const b = index.get(e.to);
    if (a === undefined || b === undefined) return [];
    const y = PAD + headH + 24 + k * ROW_H + ROW_H / 2;
    const x1 = colX(a);
    const x2 = colX(b);
    if (a === b) return [{ from: e.from, to: e.to, label: e.label, path: `M ${x1} ${y - 8} h 28 v 16 h -28`, lx: x1 + 62, ly: y + 4, anchor: 'middle' as const }];
    return [{ from: e.from, to: e.to, label: e.label, path: `M ${x1} ${y} L ${x2} ${y}`, lx: (x1 + x2) / 2, ly: y - 6, anchor: 'middle' as const }];
  });
  return {
    kind: 'sequence',
    width: PAD * 2 + Math.max(1, nodes.length) * COL_W,
    height: bottom + PAD,
    nodes: sceneNodes,
    edges: sceneEdges,
    lifelines: sceneNodes.map((n) => ({ x: r1(n.x + n.w / 2), y1: PAD + headH, y2: bottom })),
  };
}

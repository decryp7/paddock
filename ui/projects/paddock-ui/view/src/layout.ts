import type { ActionRef, Tone } from '../generated/view';

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
  const longest = Math.max(n.label.length, n.caption?.length ?? 0, ...detail.map((l) => l.length * 0.9));
  const w = Math.min(MAX_W, Math.max(MIN_W, Math.ceil(longest * CHAR_W + 28)));
  const h = 36 + (n.caption ? 12 : 0) + detail.length * LINE_H + (detail.length ? 6 : 0);
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

const r1 = (n: number): number => Math.round(n * 10) / 10;

/**
 * A layered (top-down) layout of a graph or diagram: nodes in layers by longest path from a source (cycles are broken by a depth-first
 * walk), ordered inside a layer to cut crossings, each layer centred; edges are curves from the bottom of the source to the top of the
 * target, labels at their middle. `radial` puts the layers on rings around the first layer instead. Pure and deterministic; the
 * `force` layout of the wire type is drawn as `layered`, because a force simulation is not reproducible.
 */
export function layoutGraph(nodes: readonly LayoutNode[], edges: readonly LayoutEdge[], radial = false): Scene {
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
  let height = 0;
  if (radial) {
    let r = 0;
    const centres = rows.map((row, l) => {
      const maxW = Math.max(0, ...row.map((n) => boxes[n]?.w ?? 0));
      const maxH = Math.max(0, ...row.map((n) => boxes[n]?.h ?? 0));
      if (l > 0) r = Math.max(r + maxH + 70, (row.length * (maxW + 24)) / (2 * Math.PI));
      return r;
    });
    const extent = (centres[centres.length - 1] ?? 0) + 140 + PAD;
    width = extent * 2;
    height = extent * 2;
    rows.forEach((row, l) => {
      row.forEach((n, i) => {
        const box = boxes[n] as { x: number; y: number; w: number; h: number };
        const radius = centres[l] ?? 0;
        const angle = row.length === 1 && l === 0 ? 0 : (2 * Math.PI * i) / row.length - Math.PI / 2;
        box.x = r1(extent + radius * Math.cos(angle) - box.w / 2);
        box.y = r1(extent + radius * Math.sin(angle) - box.h / 2);
      });
    });
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
  const sceneNodes: SceneNode[] = nodes.map((n, i) => {
    const b = boxes[i] as { x: number; y: number; w: number; h: number };
    return { id: n.id, label: n.label, detail: dims[i]?.detail ?? [], caption: n.caption ?? '', tone: n.tone ?? 'neutral', action: n.action ?? null, ...b };
  });
  let backs = 0;
  let reach = 0;
  const sceneEdges: SceneEdge[] = edges.flatMap((e) => {
    const a = index.get(e.from);
    const b = index.get(e.to);
    if (a === undefined || b === undefined) return [];
    const s = boxes[a] as { x: number; y: number; w: number; h: number };
    const t = boxes[b] as { x: number; y: number; w: number; h: number };
    if (a === b) {
      const x = s.x + s.w;
      const y = s.y + s.h / 2;
      return [{ from: e.from, to: e.to, label: e.label, path: `M ${r1(x)} ${r1(y - 8)} C ${r1(x + 36)} ${r1(y - 24)}, ${r1(x + 36)} ${r1(y + 24)}, ${r1(x)} ${r1(y + 8)}`, lx: r1(x + 40), ly: r1(y) }];
    }
    if (radial) {
      const [x1, y1] = boxEdge(s, t.x + t.w / 2, t.y + t.h / 2);
      const [x2, y2] = boxEdge(t, s.x + s.w / 2, s.y + s.h / 2);
      return [{ from: e.from, to: e.to, label: e.label, path: `M ${r1(x1)} ${r1(y1)} L ${r1(x2)} ${r1(y2)}`, lx: r1((x1 + x2) / 2), ly: r1((y1 + y2) / 2) }];
    }
    const down = (layer[b] ?? 0) > (layer[a] ?? 0);
    if (!down) {
      // An edge that goes back up (or sideways) runs outside the boxes, on the right, instead of through them.
      const bx = Math.max(...boxes.map((x) => x.x + x.w)) + 24 + (backs++ % 4) * 10;
      const ys = s.y + s.h / 2;
      const yt = t.y + t.h / 2;
      reach = Math.max(reach, bx);
      return [{ from: e.from, to: e.to, label: e.label, path: `M ${r1(s.x + s.w)} ${r1(ys)} H ${r1(bx)} V ${r1(yt)} H ${r1(t.x + t.w)}`, lx: r1(bx), ly: r1((ys + yt) / 2) }];
    }
    const x1 = s.x + s.w / 2;
    const y1 = down ? s.y + s.h : s.y;
    const x2 = t.x + t.w / 2;
    const y2 = down ? t.y : t.y + t.h;
    const ym = (y1 + y2) / 2;
    return [{ from: e.from, to: e.to, label: e.label, path: `M ${r1(x1)} ${r1(y1)} C ${r1(x1)} ${r1(ym)}, ${r1(x2)} ${r1(ym)}, ${r1(x2)} ${r1(y2)}`, lx: r1((x1 + x2) / 2), ly: r1(ym) }];
  });
  return { kind: radial ? 'radial' : 'layered', width: r1(Math.max(width, reach + PAD + 24)), height: r1(height), nodes: sceneNodes, edges: sceneEdges, lifelines: [] };
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
    return { id: n.id, label: n.label, detail: [], caption: n.caption ?? '', tone: n.tone ?? 'neutral', action: n.action ?? null, x: r1(colX(i) - w / 2), y: PAD, w, h: headH };
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
    if (a === b) return [{ from: e.from, to: e.to, label: e.label, path: `M ${x1} ${y - 8} h 28 v 16 h -28`, lx: x1 + 62, ly: y + 4 }];
    return [{ from: e.from, to: e.to, label: e.label, path: `M ${x1} ${y} L ${x2} ${y}`, lx: (x1 + x2) / 2, ly: y - 6 }];
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

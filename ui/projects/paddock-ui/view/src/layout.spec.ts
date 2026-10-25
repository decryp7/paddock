import { describe, expect, it } from 'vitest';
import { detailLines, hubOf, labelRect, layers, layoutGraph, layoutSequence, MARK_W, chooseSlot, nearestSlot, type LabelSlot, ringsFrom, type LayoutNode } from './layout';
import { diagramScene, graphScene } from './scene';
import { sampleViewDoc } from './sample';
import { shapeOf } from './model';
import { flattenPath, inflate, pathCrosses, type Pt } from './route';

const n = (id: string, over: Partial<LayoutNode> = {}): LayoutNode => ({ id, label: id, ...over });
const e = (from: string, to: string, label = '') => ({ from, to, label });
const overlap = (a: { x: number; y: number; w: number; h: number }, b: { x: number; y: number; w: number; h: number }): boolean =>
  a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

/** Every edge of a scene against every box that is not one of its two ends (a self loop: its own box is fine too). */
function crossings(s: ReturnType<typeof layoutGraph>): string[] {
  if (s.kind === 'sequence') return [];
  const bad: string[] = [];
  for (const edge of s.edges) {
    const others = s.nodes.filter((b) => b.id !== edge.from && b.id !== edge.to);
    if (pathCrosses(edge.path, others)) bad.push(`${edge.from}>${edge.to} (${edge.label})`);
    const pts = flattenPath(edge.path);
    for (const p of pts) if (p[0] < 0 || p[1] < 0 || p[0] > s.width || p[1] > s.height) bad.push(`${edge.from}>${edge.to} leaves the canvas at ${p}`);
  }
  return bad;
}

describe('edge routing: an edge never crosses a box (review 2, P2-1)', () => {
  const ids = (k: number, p = 'n') => Array.from({ length: k }, (_, i) => `${p}${i}`);
  const cases: { name: string; nodes: LayoutNode[]; edges: ReturnType<typeof e>[] }[] = [
    { name: 'the sample CI graph with its retry', nodes: ['a', 'b', 'c', 'd', 'e'].map((id) => n(id, { label: id === 'c' ? 'Integration tests' : id.repeat(5) })), edges: [e('a', 'b'), e('a', 'c', 'needs db'), e('b', 'd'), e('c', 'd'), e('d', 'e'), e('e', 'a', 'retry')] },
    { name: 'a long edge over a middle layer', nodes: ids(4).map((id) => n(id, { label: id.repeat(8) })), edges: [e('n0', 'n1'), e('n1', 'n2'), e('n2', 'n3'), e('n0', 'n3', 'skip'), e('n0', 'n2', 'skip two')] },
    { name: 'back edges to every layer', nodes: ids(5).map((id) => n(id, { label: id.repeat(7) })), edges: [e('n0', 'n1'), e('n1', 'n2'), e('n2', 'n3'), e('n3', 'n4'), e('n4', 'n0', 'a'), e('n4', 'n1', 'b'), e('n3', 'n0', 'c'), e('n2', 'n0', 'd'), e('n4', 'n2', 'e')] },
    { name: 'a wide row with an edge sideways inside it', nodes: ids(5).map((id) => n(id)), edges: [e('n0', 'n1'), e('n0', 'n2'), e('n0', 'n3'), e('n0', 'n4'), e('n1', 'n3'), e('n3', 'n1', 'sideways'), e('n4', 'n2')] },
    { name: 'a radial ring where neighbours on the ring link', nodes: ['c', ...ids(6, 's')].map((id) => n(id)), edges: [...ids(6, 's').map((id) => e('c', id)), e('s0', 's1'), e('s1', 's3'), e('s2', 's5'), e('s4', 's0')] },
    { name: 'a radial graph with an edge across the hub', nodes: ['c', ...ids(5, 's')].map((id) => n(id)), edges: [...ids(5, 's').map((id) => e('c', id)), e('s0', 's2'), e('s1', 's3')] },
  ];
  for (const c of cases) {
    for (const radial of [false, true]) {
      it(`${c.name}, radial=${radial}`, () => {
        expect(crossings(layoutGraph(c.nodes, c.edges, radial))).toEqual([]);
      });
    }
  }
  it('holds for the graphs and class diagram of the sample document', () => {
    const root = shapeOf(sampleViewDoc().root);
    if (root.tag !== 'stack') throw new Error('stack');
    for (const id of ['g1', 'g2', 'g3', 'd1', 'd2']) {
      const k = shapeOf(root.v.children.find((c) => c.id === id)!);
      const s = k.tag === 'graph' ? graphScene(k.v) : k.tag === 'diagram' ? diagramScene(k.v) : null;
      if (!s) throw new Error(id);
      expect(crossings(s), id).toEqual([]);
      if (k.tag === 'graph') expect(crossings(graphScene({ ...k.v, layout: k.v.layout === 'radial' ? 'layered' : 'radial' })), `${id} other layout`).toEqual([]);
    }
  });
  it('bows the spoke-to-spoke edges of the second radial sample, and they cross no box', () => {
    const root = shapeOf(sampleViewDoc().root);
    if (root.tag !== 'stack') throw new Error('stack');
    const g = shapeOf(root.v.children.find((c) => c.id === 'g3')!);
    if (g.tag !== 'graph') throw new Error('graph');
    const s = graphScene(g.v);
    const across = s.edges.filter((x) => x.path.includes('Q') || x.path.includes('C'));
    expect(across.length).toBeGreaterThanOrEqual(1);
    // The shell to chromium edge bows too (review 4, P3-1), so it does not cross a spoke into the hub.
    expect(s.edges.find((x) => x.from === 'p2' && x.to === 'p5')!.path).toContain(' Q ');
    expect(crossings(s)).toEqual([]);
  });
  it('sends the retry edge around the outside: right of every box, with a vertical lane', () => {
    const root = shapeOf(sampleViewDoc().root);
    if (root.tag !== 'stack') throw new Error('stack');
    const g = shapeOf(root.v.children.find((c) => c.id === 'g1')!);
    if (g.tag !== 'graph') throw new Error('graph');
    const s = graphScene(g.v);
    const retry = s.edges.find((x) => x.label === 'retry')!;
    const right = Math.max(...s.nodes.map((b) => b.x + b.w));
    expect(Math.max(...flattenPath(retry.path).map((p) => p[0]))).toBeGreaterThan(right);
    expect(retry.lx).toBeGreaterThan(right);
  });
});

type Scn = ReturnType<typeof layoutGraph>;

/** Labels that touch a box or any edge path (their own included), with 1 unit of clearance. */
function labelHits(s: Scn): string[] {
  if (s.kind === 'sequence') return [];
  const bad: string[] = [];
  for (const edge of s.edges) {
    if (!edge.label) continue;
    const r = inflate(labelRect(edge.lx, edge.ly, edge.anchor, edge.label), 1);
    if (r.x < 0 || r.y < 0 || r.x + r.w > s.width || r.y + r.h > s.height) bad.push(`${edge.label} leaves the canvas`);
    for (const b of s.nodes) if (overlap(r, b)) bad.push(`${edge.label} touches box ${b.id}`);
    for (const o of s.edges) if (pathCrosses(o.path, [r])) bad.push(`${edge.label} touches edge ${o.from}>${o.to}`);
  }
  return bad;
}

/** Whether two segments properly cross (a shared end point does not count). */
function cross(a: Pt, b: Pt, c: Pt, d: Pt): boolean {
  const o = (p: Pt, q: Pt, r: Pt): number => Math.sign((q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]));
  return o(a, b, c) * o(a, b, d) < 0 && o(c, d, a) * o(c, d, b) < 0;
}

/** Edges that cross another edge within their last 40 units (the arrowhead). */
function arrowCrossings(s: Scn): string[] {
  const bad: string[] = [];
  for (const e1 of s.edges) {
    const pts = flattenPath(e1.path);
    const tail: Pt[] = [];
    let len = 0;
    for (let i = pts.length - 1; i > 0 && len < 40; i--) {
      tail.push(pts[i] as Pt);
      len += Math.hypot(pts[i]![0] - pts[i - 1]![0], pts[i]![1] - pts[i - 1]![1]);
      if (len >= 40 || i === 1) tail.push(pts[i - 1] as Pt);
    }
    for (const e2 of s.edges) {
      if (e1 === e2) continue;
      const q = flattenPath(e2.path);
      for (let i = 1; i < tail.length; i++) for (let j = 1; j < q.length; j++) if (cross(tail[i - 1] as Pt, tail[i] as Pt, q[j - 1] as Pt, q[j] as Pt)) bad.push(`${e1.from}>${e1.to} crosses ${e2.from}>${e2.to} near its arrowhead`);
    }
  }
  return [...new Set(bad)];
}

function sampleScenes(): { id: string; scene: Scn }[] {
  const root = shapeOf(sampleViewDoc().root);
  if (root.tag !== 'stack') throw new Error('stack');
  const out: { id: string; scene: Scn }[] = [];
  for (const id of ['g1', 'g2', 'g3', 'd1', 'd2']) {
    const k = shapeOf(root.v.children.find((c) => c.id === id)!);
    if (k.tag === 'graph') {
      out.push({ id, scene: graphScene(k.v) }, { id: `${id} other layout`, scene: graphScene({ ...k.v, layout: k.v.layout === 'radial' ? 'layered' : 'radial' }) });
    } else if (k.tag === 'diagram') out.push({ id, scene: diagramScene(k.v) });
  }
  return out;
}

describe('edge labels (review 4, P2-1, P3-1)', () => {
  const cases = [
    { name: 'two spokes joined by a labelled edge', nodes: ['c', 'a', 'b', 'd'].map((id) => n(id)), edges: [e('a', 'c'), e('b', 'c'), e('d', 'c'), e('a', 'b', 'tokens')] },
    { name: 'a labelled spoke across the hub', nodes: ['c', 'a', 'b', 'd', 'f', 'g'].map((id) => n(id)), edges: [e('a', 'c'), e('b', 'c'), e('d', 'c'), e('f', 'c'), e('g', 'c'), e('a', 'f', 'across the hub')] },
    { name: 'labels on every edge of a ring', nodes: ['c', ...Array.from({ length: 6 }, (_, i) => `s${i}`)].map((id) => n(id)), edges: [...Array.from({ length: 6 }, (_, i) => e('c', `s${i}`, `link ${i}`)), e('s0', 's1', 'next'), e('s2', 's5', 'far')] },
  ];
  for (const c of cases) {
    for (const radial of [false, true]) {
      it(`no label touches a box or an edge: ${c.name}, radial=${radial}`, () => {
        expect(labelHits(layoutGraph(c.nodes, c.edges, radial))).toEqual([]);
      });
    }
  }
  it('no label touches a box or an edge in the sample graphs, in both layouts', () => {
    for (const { id, scene } of sampleScenes()) expect(labelHits(scene), id).toEqual([]);
  });
  it('no label touches a box or an edge in the graphs of the routing cases', () => {
    const ids = (k: number, p = 'n') => Array.from({ length: k }, (_, i) => `${p}${i}`);
    const graphs = [
      { nodes: ['a', 'b', 'c', 'd', 'e'].map((id) => n(id, { label: id === 'c' ? 'Integration tests' : id.repeat(5) })), edges: [e('a', 'b'), e('a', 'c', 'needs db'), e('b', 'd'), e('c', 'd'), e('d', 'e'), e('e', 'a', 'retry')] },
      { nodes: ids(4).map((id) => n(id, { label: id.repeat(8) })), edges: [e('n0', 'n1'), e('n1', 'n2'), e('n2', 'n3'), e('n0', 'n3', 'skip'), e('n0', 'n2', 'skip two')] },
      { nodes: ['c', ...ids(6, 's')].map((id) => n(id)), edges: [...ids(6, 's').map((id) => e('c', id)), e('s0', 's1', 'x'), e('s1', 's3', 'y'), e('s2', 's5', 'z'), e('s4', 's0', 'w')] },
      { nodes: ['c', ...ids(5, 's')].map((id) => n(id)), edges: [...ids(5, 's').map((id) => e('c', id)), e('s0', 's2', 'across'), e('s1', 's3', 'across too')] },
    ];
    graphs.forEach((g, i) => {
      for (const radial of [false, true]) expect(labelHits(layoutGraph(g.nodes, g.edges, radial)), `graph ${i} radial=${radial}`).toEqual([]);
    });
  });
  it('no edge of the radial sample graphs crosses another within its last segment (the arrowhead)', () => {
    for (const { id, scene } of sampleScenes().filter((x) => x.scene.kind === 'radial')) expect(arrowCrossings(scene), id).toEqual([]);
  });
});

/** Distance from a point to a rectangle (0 inside). */
const pointRect = (p: Pt, r: { x: number; y: number; w: number; h: number }): number => Math.hypot(Math.max(r.x - p[0], 0, p[0] - (r.x + r.w)), Math.max(r.y - p[1], 0, p[1] - (r.y + r.h)));
/** Distance from a rectangle to a path, sampled every half unit along each segment. */
function rectPath(r: { x: number; y: number; w: number; h: number }, path: string): number {
  const pts = flattenPath(path);
  let best = Infinity;
  for (let i = 1; i < pts.length; i++) {
    const [a, b] = [pts[i - 1] as Pt, pts[i] as Pt];
    const steps = Math.max(1, Math.ceil(Math.hypot(b[0] - a[0], b[1] - a[1]) * 2));
    for (let k = 0; k <= steps; k++) best = Math.min(best, pointRect([a[0] + ((b[0] - a[0]) * k) / steps, a[1] + ((b[1] - a[1]) * k) / steps], r));
  }
  return best;
}
const rectRect = (a: { x: number; y: number; w: number; h: number }, b: { x: number; y: number; w: number; h: number }): number =>
  Math.hypot(Math.max(b.x - (a.x + a.w), a.x - (b.x + b.w), 0), Math.max(b.y - (a.y + a.h), a.y - (b.y + b.h), 0));

/** Labels closer than `gap` units to a box or to any edge path (their own included). */
function labelGaps(s: Scn, gap: number): string[] {
  if (s.kind === 'sequence') return [];
  const bad: string[] = [];
  for (const edge of s.edges) {
    if (!edge.label) continue;
    const r = labelRect(edge.lx, edge.ly, edge.anchor, edge.label);
    for (const b of s.nodes) if (rectRect(r, b) < gap) bad.push(`${edge.label} is ${rectRect(r, b).toFixed(1)} from box ${b.id}`);
    for (const o of s.edges) if (rectPath(r, o.path) < gap) bad.push(`${edge.label} is ${rectPath(r, o.path).toFixed(1)} from edge ${o.from}>${o.to}`);
  }
  return bad;
}

function allGraphs(): { id: string; scene: Scn }[] {
  const ids = (k: number, p = 'n') => Array.from({ length: k }, (_, i) => `${p}${i}`);
  const graphs = [
    { nodes: ['a', 'b', 'c', 'd', 'e'].map((id) => n(id, { label: id === 'c' ? 'Integration tests' : id.repeat(5) })), edges: [e('a', 'b'), e('a', 'c', 'needs db'), e('b', 'd'), e('c', 'd'), e('d', 'e'), e('e', 'a', 'retry')] },
    { nodes: ids(4).map((id) => n(id, { label: id.repeat(8) })), edges: [e('n0', 'n1'), e('n1', 'n2'), e('n2', 'n3'), e('n0', 'n3', 'skip'), e('n0', 'n2', 'skip two')] },
    { nodes: ['c', ...ids(6, 's')].map((id) => n(id)), edges: [...ids(6, 's').map((id) => e('c', id, `link ${id}`)), e('s0', 's1', 'x'), e('s1', 's3', 'y'), e('s2', 's5', 'z'), e('s4', 's0', 'w')] },
    { nodes: ['c', ...ids(5, 's')].map((id) => n(id)), edges: [...ids(5, 's').map((id) => e('c', id)), e('s0', 's2', 'across'), e('s1', 's3', 'across too')] },
  ];
  const out = sampleScenes();
  graphs.forEach((g, i) => {
    for (const radial of [false, true]) out.push({ id: `graph ${i} radial=${radial}`, scene: layoutGraph(g.nodes, g.edges, radial) });
  });
  return out;
}

describe('label gap and slot choice (review 5, P3)', () => {
  it('keeps every label at least 4 units from every box and every edge, in all graphs and both layouts', () => {
    for (const { id, scene } of allGraphs()) expect(labelGaps(scene, 4), id).toEqual([]);
  });
  it('puts "needs db" more than 4 units from its curve', () => {
    const s = layoutGraph(['a', 'b', 'c', 'd', 'e'].map((id) => n(id, { label: id === 'c' ? 'Integration tests' : id.repeat(5) })), [e('a', 'b'), e('a', 'c', 'needs db'), e('b', 'd'), e('c', 'd'), e('d', 'e'), e('e', 'a', 'retry')]);
    const edge = s.kind === 'sequence' ? null : s.edges.find((x) => x.label === 'needs db')!;
    expect(rectPath(labelRect(edge!.lx, edge!.ly, edge!.anchor, edge!.label), edge!.path)).toBeGreaterThanOrEqual(4);
  });
  it('picks the nearer of two clear slots, and the earlier one on a tie', () => {
    const far = { lx: 100, ly: 50, anchor: 'middle' as const };
    const near = { lx: 60, ly: 50, anchor: 'middle' as const };
    expect(nearestSlot([far, near], 'abc', [50, 46])).toBe(near);
    expect(nearestSlot([near, far], 'abc', [50, 46])).toBe(near);
    const left = { lx: 40, ly: 50, anchor: 'middle' as const };
    const right = { lx: 60, ly: 50, anchor: 'middle' as const };
    expect(nearestSlot([right, left], 'abc', [50, 46])).toBe(right);
    expect(nearestSlot([], 'abc', [0, 0])).toBeNull();
  });
  it('falls back to a slot clear at 1 unit, then to the one with the fewest hits', () => {
    const [a, b, c] = [60, 100, 140].map((lx): LabelSlot => ({ lx, ly: 50, anchor: 'middle' }));
    const mid: [number, number] = [100, 46];
    // None clear at 4, two clear at 1: the nearer of those.
    expect(chooseSlot([a!, b!, c!], 'abc', mid, (s, gap) => (gap === 4 ? 1 : s === a ? 1 : 0))).toBe(b);
    // None clear at all: the fewest hits at 1 unit, the first on a tie.
    const counts = new Map<LabelSlot, number>([[a!, 3], [b!, 2], [c!, 2]]);
    expect(chooseSlot([a!, b!, c!], 'abc', mid, (s) => counts.get(s)!)).toBe(b);
  });
  it('puts the label of a straight edge at the clear slot nearest its middle', () => {
    const s = layoutGraph([n('a'), n('b')], [e('a', 'b', 'link')], true);
    if (s.kind === 'sequence') throw new Error('kind');
    const edge = s.edges[0]!;
    const pts = flattenPath(edge.path);
    const mid: Pt = [(pts[0]![0] + pts[1]![0]) / 2, (pts[0]![1] + pts[1]![1]) / 2];
    const r = labelRect(edge.lx, edge.ly, edge.anchor, edge.label);
    // Beside the line, 4 units clear: the centre is half the extent across plus 4 (+ rounding) from the middle of the line.
    const d = Math.hypot(r.x + r.w / 2 - mid[0], r.y + r.h / 2 - mid[1]);
    expect(d).toBeLessThanOrEqual(Math.max(r.w, r.h) / 2 + 4 + 3);
  });
});

describe('the radial canvas fits the drawing (review 5, P3)', () => {
  /** The bounds of everything drawn: boxes, edge paths (curves flattened) and labels. */
  function drawn(s: Scn): { minX: number; minY: number; maxX: number; maxY: number } {
    if (s.kind === 'sequence') throw new Error('kind');
    const b = { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity };
    const grow = (x0: number, y0: number, x1: number, y1: number): void => {
      b.minX = Math.min(b.minX, x0);
      b.minY = Math.min(b.minY, y0);
      b.maxX = Math.max(b.maxX, x1);
      b.maxY = Math.max(b.maxY, y1);
    };
    for (const x of s.nodes) grow(x.x, x.y, x.x + x.w, x.y + x.h);
    for (const edge of s.edges) {
      for (const p of flattenPath(edge.path)) grow(p[0], p[1], p[0], p[1]);
      if (edge.label) {
        const r = labelRect(edge.lx, edge.ly, edge.anchor, edge.label);
        grow(r.x, r.y, r.x + r.w, r.y + r.h);
      }
    }
    return b;
  }
  const radials = (): { id: string; scene: Scn }[] => allGraphs().filter((x) => x.scene.kind === 'radial');
  it('has as many radial graphs to check as the samples and cases give', () => {
    expect(radials().length).toBeGreaterThanOrEqual(6);
  });
  it('is the drawn bounds plus the padding of the other layouts, with no extra margin', () => {
    const pad = 16;
    for (const { id, scene } of radials()) {
      const b = drawn(scene);
      expect(Math.abs(b.minX - pad), `${id} left`).toBeLessThanOrEqual(1);
      expect(Math.abs(b.minY - pad), `${id} top`).toBeLessThanOrEqual(1);
      expect(Math.abs(scene.width - (b.maxX + pad)), `${id} width`).toBeLessThanOrEqual(1);
      expect(Math.abs(scene.height - (b.maxY + pad)), `${id} height`).toBeLessThanOrEqual(1);
    }
  });
  it('draws nothing outside the canvas', () => {
    for (const { id, scene } of radials()) {
      const b = drawn(scene);
      expect(b.minX, id).toBeGreaterThanOrEqual(0);
      expect(b.minY, id).toBeGreaterThanOrEqual(0);
      expect(b.maxX, id).toBeLessThanOrEqual(scene.width);
      expect(b.maxY, id).toBeLessThanOrEqual(scene.height);
    }
  });
});

describe('layers', () => {
  it('puts a node below the longest path to it', () => {
    expect(layers(4, [[0, 1], [1, 2], [0, 2], [0, 3]])).toEqual([0, 1, 2, 1]);
  });
  it('breaks a cycle instead of looping and ignores self loops', () => {
    expect(layers(3, [[0, 1], [1, 2], [2, 0], [1, 1]])).toEqual([0, 1, 2]);
    expect(layers(0, [])).toEqual([]);
  });
  it('is deterministic', () => {
    const edges: [number, number][] = [[3, 1], [1, 0], [0, 2], [2, 3]];
    expect(layers(4, edges)).toEqual(layers(4, edges));
  });
});

describe('layoutGraph', () => {
  it('draws a layered graph top-down without overlapping boxes, inside its size', () => {
    const nodes = ['a', 'b', 'c', 'd', 'e'].map((id) => n(id, { label: id.repeat(12) }));
    const s = layoutGraph(nodes, [e('a', 'b'), e('a', 'c'), e('b', 'd'), e('c', 'd', 'x'), e('d', 'e'), e('e', 'a')]);
    expect(s.kind).toBe('layered');
    for (let i = 0; i < s.nodes.length; i++) {
      const a = s.nodes[i]!;
      expect(a.x).toBeGreaterThanOrEqual(0);
      expect(a.x + a.w).toBeLessThanOrEqual(s.width);
      expect(a.y + a.h).toBeLessThanOrEqual(s.height);
      for (let j = i + 1; j < s.nodes.length; j++) expect(overlap(a, s.nodes[j]!)).toBe(false);
    }
    const y = (id: string) => s.nodes.find((x) => x.id === id)!.y;
    expect(y('a')).toBeLessThan(y('b'));
    expect(y('d')).toBeLessThan(y('e'));
    expect(s.edges).toHaveLength(6);
    expect(s.edges.find((x) => x.label === 'x')).toBeTruthy();
    // The cycle edge runs outside the boxes, on the right.
    const back = s.edges.find((x) => x.from === 'e' && x.to === 'a')!;
    expect(back.path).toMatch(/^M [\d.]+ [\d.]+( L [\d.]+ [\d.]+| Q [\d.]+ [\d.]+ [\d.]+ [\d.]+)+$/);
    expect(Math.min(...back.path.match(/[\d.]+/g)!.filter((_, i) => i % 2 === 0).map(Number))).toBeGreaterThan(0);
    expect(back.lx).toBeLessThanOrEqual(s.width);
    expect(back.anchor).toBe('start');
  });

  it('is the same every time', () => {
    const nodes = ['a', 'b', 'c'].map((id) => n(id));
    const edges = [e('a', 'b'), e('a', 'c')];
    expect(layoutGraph(nodes, edges)).toEqual(layoutGraph(nodes, edges));
  });

  it('draws a self loop, skips edges to unknown nodes and handles no nodes', () => {
    const s = layoutGraph([n('a')], [e('a', 'a', 'again'), e('a', 'zz')]);
    expect(s.edges).toHaveLength(1);
    expect(s.edges[0]!.lx).toBeGreaterThan(s.nodes[0]!.x + s.nodes[0]!.w);
    const none = layoutGraph([], []);
    expect(none.nodes).toEqual([]);
    expect(none.width).toBeGreaterThan(0);
  });

  it('draws a radial graph around the first layer with straight edges that end at the boxes', () => {
    const nodes = ['hub', 'p1', 'p2', 'p3', 'p4'].map((id) => n(id));
    const s = layoutGraph(nodes, [e('p1', 'hub'), e('p2', 'hub'), e('p3', 'hub'), e('p4', 'hub'), e('hub', 'hub')], true);
    expect(s.kind).toBe('radial');
    const hub = s.nodes.find((x) => x.id === 'hub')!;
    const p1 = s.nodes.find((x) => x.id === 'p1')!;
    expect(Math.abs(hub.x + hub.w / 2 - p1.x - p1.w / 2) + Math.abs(hub.y + hub.h / 2 - p1.y - p1.h / 2)).toBeGreaterThan(50);
    expect(s.edges.some((x) => x.path.includes(' L '))).toBe(true);
    expect(hub.x + hub.w / 2).toBeGreaterThan(p1.x + p1.w / 2 - 400);
    for (const x of s.nodes) {
      expect(x.x).toBeGreaterThanOrEqual(0);
      expect(x.y).toBeGreaterThanOrEqual(0);
    }
  });

  it('puts the hub of a radial graph in the middle and the spokes on a ring around it (review 1, P1-1)', () => {
    const s = layoutGraph(['hub', 'a', 'b', 'c'].map((id) => n(id)), [e('a', 'hub'), e('b', 'hub'), e('c', 'hub')], true);
    const centre = (id: string) => {
      const x = s.nodes.find((q) => q.id === id)!;
      return [x.x + x.w / 2, x.y + x.h / 2] as const;
    };
    const [hx, hy] = centre('hub');
    const radii = ['a', 'b', 'c'].map((id) => Math.hypot(centre(id)[0] - hx, centre(id)[1] - hy));
    expect(radii[0]).toBeGreaterThan(50);
    expect(Math.max(...radii) - Math.min(...radii)).toBeLessThan(0.5);
    // The canvas fits the boxes: no more than the padding around the real bounding box.
    const left = Math.min(...s.nodes.map((q) => q.x));
    const right = Math.max(...s.nodes.map((q) => q.x + q.w));
    const top = Math.min(...s.nodes.map((q) => q.y));
    const bottom = Math.max(...s.nodes.map((q) => q.y + q.h));
    expect([left, top]).toEqual([16, 16]);
    expect(s.width).toBeCloseTo(right + 16, 0);
    expect(s.height).toBeCloseTo(bottom + 16, 0);
  });

  it('never puts two boxes on each other, in any layout: several roots, several components, chains, hubs, one ring of many', () => {
    const ids = (k: number, p = 'n') => Array.from({ length: k }, (_, i) => `${p}${i}`);
    const cases: { name: string; nodes: LayoutNode[]; edges: ReturnType<typeof e>[] }[] = [
      { name: 'three roots into one hub', nodes: ['hub', 'x', 'y', 'z'].map((id) => n(id)), edges: [e('x', 'hub'), e('y', 'hub'), e('z', 'hub')] },
      { name: 'two disconnected components', nodes: ids(6).map((id) => n(id)), edges: [e('n0', 'n1'), e('n1', 'n2'), e('n3', 'n4'), e('n4', 'n5')] },
      { name: 'no edges at all', nodes: ids(9).map((id) => n(id, { label: id.repeat(6) })), edges: [] },
      { name: 'a chain', nodes: ids(7).map((id) => n(id)), edges: ids(6).map((id, i) => e(id, `n${i + 1}`)) },
      { name: 'a star of twelve with long labels and details', nodes: [n('c', { detail: 'a\nb\nc', caption: 'g' }), ...ids(12, 's').map((id) => n(id, { label: id.repeat(9), tone: 'bad' }))], edges: ids(12, 's').map((id) => e('c', id, 'edge')) },
      { name: 'a cycle', nodes: ids(5).map((id) => n(id)), edges: [e('n0', 'n1'), e('n1', 'n2'), e('n2', 'n3'), e('n3', 'n4'), e('n4', 'n0')] },
      { name: 'self loops and unknown ends', nodes: ids(3).map((id) => n(id)), edges: [e('n0', 'n0', 'loop'), e('n0', 'zz'), e('n1', 'n2')] },
    ];
    for (const c of cases) {
      for (const radial of [false, true]) {
        const s = layoutGraph(c.nodes, c.edges, radial);
        for (let i = 0; i < s.nodes.length; i++) {
          const a = s.nodes[i]!;
          expect(a.x, `${c.name} radial=${radial} ${a.id}`).toBeGreaterThanOrEqual(0);
          expect(a.y).toBeGreaterThanOrEqual(0);
          expect(a.x + a.w).toBeLessThanOrEqual(s.width);
          expect(a.y + a.h).toBeLessThanOrEqual(s.height);
          for (let j = i + 1; j < s.nodes.length; j++) expect(overlap(a, s.nodes[j]!), `${c.name} radial=${radial} ${a.id}/${s.nodes[j]!.id}`).toBe(false);
        }
      }
    }
  });

  it('does not overlap the boxes of the sample documents graphs either', () => {
    const root = shapeOf(sampleViewDoc().root);
    if (root.tag !== 'stack') throw new Error('stack');
    for (const id of ['g1', 'g2', 'g3', 'd1', 'd2']) {
      const k = shapeOf(root.v.children.find((c) => c.id === id)!);
      const s = k.tag === 'graph' ? graphScene(k.v) : k.tag === 'diagram' ? diagramScene(k.v) : null;
      if (!s) throw new Error(id);
      for (let i = 0; i < s.nodes.length; i++) for (let j = i + 1; j < s.nodes.length; j++) expect(overlap(s.nodes[i]!, s.nodes[j]!), id).toBe(false);
    }
  });

  it('finds the hub and the rings from it, and puts what it cannot reach one ring out', () => {
    expect(hubOf(4, [[1, 0], [2, 0], [3, 0]])).toBe(0);
    expect(hubOf(3, [])).toBe(0);
    expect(hubOf(3, [[1, 1], [1, 2]])).toBe(1);
    expect(ringsFrom(0, 5, [[1, 0], [2, 1], [3, 3]])).toEqual([0, 1, 2, 3, 3]);
  });

  it('keeps the label of an edge beside its line, not on it', () => {
    const s = layoutGraph(['a', 'b'].map((id) => n(id)), [e('a', 'b', 'needs db')]);
    const x1 = s.nodes[0]!.x + s.nodes[0]!.w / 2;
    const r = labelRect(s.edges[0]!.lx, s.edges[0]!.ly, s.edges[0]!.anchor, 'needs db');
    expect(r.x).toBeGreaterThanOrEqual(x1 + 4);
  });

  it('makes room for the status mark of a box', () => {
    const [plain, marked] = layoutGraph([n('a', { label: 'x'.repeat(14) }), n('b', { label: 'x'.repeat(14), tone: 'good' })], []).nodes;
    expect(marked!.w).toBe(plain!.w + MARK_W);
  });

  it('puts the one root of a radial graph in the middle', () => {
    const s = layoutGraph([n('a')], [], true);
    expect(s.nodes[0]!.x + s.nodes[0]!.w / 2).toBeCloseTo(s.width / 2, 0);
  });

  it('sizes boxes by their text, bounded', () => {
    const s = layoutGraph([n('a'), n('b', { label: 'x'.repeat(200), detail: 'one\n\ntwo\n' + 'y'.repeat(100), caption: 'group' })], []);
    const [a, b] = s.nodes;
    expect(a!.w).toBe(96);
    expect(b!.w).toBe(240);
    expect(b!.detail).toHaveLength(3);
    expect(b!.detail[2]!.endsWith('…')).toBe(true);
    expect(b!.h).toBeGreaterThan(a!.h);
  });
});

describe('layoutSequence', () => {
  it('puts participants left to right and messages top to bottom in the order given', () => {
    const s = layoutSequence([n('a'), n('b'), n('c')], [e('a', 'b', 'one'), e('b', 'c', 'two'), e('c', 'c', 'self'), e('c', 'a', 'back'), e('a', 'zz')]);
    expect(s.kind).toBe('sequence');
    expect(s.lifelines).toHaveLength(3);
    expect(s.nodes.map((x) => x.id)).toEqual(['a', 'b', 'c']);
    expect(s.nodes[0]!.x).toBeLessThan(s.nodes[1]!.x);
    expect(s.edges).toHaveLength(4);
    expect(s.edges[0]!.ly).toBeLessThan(s.edges[1]!.ly);
    expect(s.edges[2]!.path).toContain(' h 28 v 16 h -28');
    expect(s.edges[3]!.path).toMatch(/^M \d+ [\d.]+ L \d+ /);
    expect(s.lifelines[0]!.y2).toBeLessThanOrEqual(s.height);
  });
  it('handles no participants', () => {
    expect(layoutSequence([], []).width).toBeGreaterThan(0);
  });
});

describe('detailLines', () => {
  it('splits, drops blank lines and bounds lines and count', () => {
    expect(detailLines(undefined)).toEqual([]);
    expect(detailLines('')).toEqual([]);
    expect(detailLines('a\r\n\r\nb')).toEqual(['a', 'b']);
    expect(detailLines(Array.from({ length: 20 }, (_, i) => `l${i}`).join('\n'))).toHaveLength(12);
  });
});

describe('scenes of the sample document', () => {
  it('draws the graph, the radial graph and the diagrams of the sample', () => {
    const find = (id: string) => {
      const root = shapeOf(sampleViewDoc().root);
      if (root.tag !== 'stack') throw new Error('stack');
      return shapeOf(root.v.children.find((c) => c.id === id)!);
    };
    const g1 = find('g1');
    const g2 = find('g2');
    const d1 = find('d1');
    const d2 = find('d2');
    if (g1.tag !== 'graph' || g2.tag !== 'graph' || d1.tag !== 'diagram' || d2.tag !== 'diagram') throw new Error('kinds');
    expect(graphScene(g1.v).kind).toBe('layered');
    expect(graphScene(g2.v).kind).toBe('radial');
    expect(diagramScene(d1.v).nodes[0]!.detail).toHaveLength(3);
    expect(diagramScene(d2.v).kind).toBe('sequence');
    const forceAsLayered = graphScene({ ...g1.v, layout: 'force' });
    expect(forceAsLayered.kind).toBe('layered');
  });
});

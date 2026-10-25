import { describe, expect, it } from 'vitest';
import { detailLines, layers, layoutGraph, layoutSequence, type LayoutNode } from './layout';
import { diagramScene, graphScene } from './scene';
import { sampleViewDoc } from './sample';
import { shapeOf } from './model';

const n = (id: string, over: Partial<LayoutNode> = {}): LayoutNode => ({ id, label: id, ...over });
const e = (from: string, to: string, label = '') => ({ from, to, label });
const overlap = (a: { x: number; y: number; w: number; h: number }, b: { x: number; y: number; w: number; h: number }): boolean =>
  a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

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
    expect(back.path).toMatch(/^M [\d.]+ [\d.]+ H [\d.]+ V [\d.]+ H [\d.]+$/);
    expect(back.lx).toBeLessThanOrEqual(s.width);
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
    for (const x of s.nodes) {
      expect(x.x).toBeGreaterThanOrEqual(0);
      expect(x.y).toBeGreaterThanOrEqual(0);
    }
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

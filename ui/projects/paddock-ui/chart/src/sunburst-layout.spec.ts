import { describe, expect, it } from 'vitest';
import {
  arcPath,
  branchSlots,
  clampOr,
  contrastRatio,
  findTrail,
  labelFits,
  labelPath,
  luminance,
  nextFocus,
  ringRadii,
  sizeOf,
  sunburstLayout,
  textWidth,
  type PdSunburstNode,
} from './sunburst-layout';

const leaf = (id: string, value: number, extra: Partial<PdSunburstNode> = {}): PdSunburstNode => ({ id, label: id.toUpperCase(), value, ...extra });
const TREE: PdSunburstNode = {
  id: 'root',
  label: 'Root',
  value: 100,
  children: [
    leaf('a', 40, { children: [leaf('a1', 25), leaf('a2', 15)] }),
    leaf('b', 30, { children: [leaf('b1', 30)] }),
    leaf('c', 10),
    leaf('hid', 5, { kind: 'hidden' }),
    leaf('pend', 5, { kind: 'pending' }),
    leaf('oth', 4, { kind: 'other' }),
    leaf('d', 3),
    leaf('e', 3),
  ],
};
const ORDER = [0, 1, 2, 3, 4];

describe('helpers', () => {
  it('sizes and clamps odd numbers', () => {
    expect(sizeOf(NaN)).toBe(0);
    expect(sizeOf(-1)).toBe(0);
    expect(sizeOf(Infinity)).toBe(0);
    expect(sizeOf(7)).toBe(7);
    expect(clampOr(NaN, 1, 6, 4)).toBe(4);
    expect(clampOr(99, 1, 6, 4)).toBe(6);
    expect(clampOr(-3, 1, 6, 4)).toBe(1);
  });

  it('gives ring-ordered slots', () => {
    expect(branchSlots(['#60aeeb', '#f3aa25', '#3dd39d', '#d68cc2', '#f07d3a', '#7a70ff', '#e8dc5a']).length).toBe(5);
    expect(branchSlots([])).toEqual([0, 1, 2, 3, 4]);
  });

  it('finds a trail', () => {
    expect(findTrail(TREE, null).map((n) => n.id)).toEqual(['root']);
    expect(findTrail(TREE, 'root').map((n) => n.id)).toEqual(['root']);
    expect(findTrail(TREE, 'a2').map((n) => n.id)).toEqual(['root', 'a', 'a2']);
    expect(findTrail(TREE, 'nope').map((n) => n.id)).toEqual(['root']);
  });
});

describe('sunburstLayout', () => {
  const l = sunburstLayout(TREE, null, { order: ORDER });

  it('places children clockwise from 12 o clock, sizes adding up per parent', () => {
    const ring1 = l.arcs.filter((a) => a.depth === 1);
    expect(ring1[0]?.id).toBe('a');
    expect(ring1[0]?.a0).toBe(0);
    expect(ring1[0]?.a1).toBeCloseTo(144);
    for (let i = 1; i < ring1.length; i += 1) expect(ring1[i]?.a0).toBeCloseTo(ring1[i - 1]?.a1 ?? -1);
    expect(ring1[ring1.length - 1]?.a1).toBeCloseTo(360);
    const a = l.arcs.find((x) => x.id === 'a');
    const kids = l.arcs.filter((x) => x.parent === 'a');
    expect(kids.reduce((s, k) => s + (k.a1 - k.a0), 0)).toBeCloseTo((a?.a1 ?? 0) - (a?.a0 ?? 0));
    expect(kids[0]?.a0).toBe(0);
    expect(a?.open).toBe(true);
    expect(a?.expandable).toBe(true);
    expect(l.scale).toBe(100);
    expect(l.folded).toBeNull();
  });

  it('orders items by size, then consumer kinds, and colours the branches', () => {
    expect(l.arcs.filter((a) => a.depth === 1).map((a) => a.id)).toEqual(['a', 'b', 'c', 'd', 'e', 'oth', 'hid', 'pend']);
    expect(l.arcs.filter((a) => a.depth === 1).map((a) => a.slot)).toEqual([0, 1, 2, 3, 4, -1, -1, -1]);
    expect(l.arcs.find((a) => a.id === 'a1')?.slot).toBe(0);
    expect(l.arcs.find((a) => a.id === 'a1')?.tone).toBe(0);
    expect(l.arcs.find((a) => a.id === 'a2')?.tone).toBe(1);
    expect(l.arcs.find((a) => a.id === 'a')?.posinset).toBe(1);
    expect(l.arcs.find((a) => a.id === 'a')?.setsize).toBe(8);
  });

  it('the sixth branch is neutral', () => {
    const six: PdSunburstNode = { id: 'r', label: 'R', value: 60, children: ['1', '2', '3', '4', '5', '6'].map((i) => leaf(i, 10)) };
    const s = sunburstLayout(six, null, { order: ORDER });
    expect(s.arcs.map((a) => a.slot)).toEqual([0, 1, 2, 3, 4, -1]);
    expect(sunburstLayout(six, null).arcs[0]?.slot).toBe(0);
  });

  it('colours by identity with a function', () => {
    const s = sunburstLayout(TREE, null, { colourBy: (n) => (n.id === 'a' ? 6.7 : n.id === 'b' ? NaN : 'neutral') });
    expect(s.arcs.find((a) => a.id === 'a')?.slot).toBe(6);
    expect(s.arcs.find((a) => a.id === 'b')?.slot).toBe(-1);
    expect(s.arcs.find((a) => a.id === 'c')?.slot).toBe(-1);
  });

  it('folds small items by angle into one sector per parent', () => {
    const s = sunburstLayout(TREE, null, { minAngle: 20, order: ORDER });
    const folded = s.arcs.find((a) => a.kind === 'folded');
    expect(folded?.label).toBe('2 smaller items');
    expect(folded?.foldedIds).toEqual(['d', 'e']);
    expect(folded?.slot).toBe(-1);
    expect(folded?.parentNodeId).toBe('root');
    expect(s.arcs.some((a) => a.id === 'd')).toBe(false);
    const one = sunburstLayout({ id: 'r', label: 'R', value: 100, children: [leaf('x', 99), leaf('y', 1)] }, null, { minAngle: 5 });
    expect(one.arcs.find((a) => a.kind === 'folded')?.label).toBe('1 smaller item');
  });

  it('folds by count', () => {
    const many: PdSunburstNode = { id: 'r', label: 'R', value: 1000, children: Array.from({ length: 10 }, (_, i) => leaf(`k${i}`, 100)) };
    const s = sunburstLayout(many, null, { maxChildren: 3 });
    expect(s.arcs.map((a) => a.kind)).toEqual(['item', 'item', 'item', 'folded']);
    expect(s.arcs[3]?.foldedIds.length).toBe(7);
  });

  it('raises the minimum angle until the segment cap fits', () => {
    const wide: PdSunburstNode = { id: 'r', label: 'R', value: 0, children: [] };
    const kids = Array.from({ length: 400 }, (_, i) => leaf(`k${i}`, 10 + i));
    const root = { ...wide, value: kids.reduce((s, k) => s + k.value, 0), children: kids };
    const s = sunburstLayout(root, null, { maxChildren: 1000, minAngle: 1, maxSegments: 20 });
    expect(s.arcs.length).toBeLessThanOrEqual(20);
    expect(s.folded?.minAngle).toBeGreaterThan(1);
    expect(s.folded?.segments).toBe(s.arcs.length);
    const hard = sunburstLayout(root, null, { maxChildren: 1000, minAngle: 1, maxSegments: 99999 });
    expect(hard.arcs.length).toBeLessThanOrEqual(2000);
  });

  it('stops raising at a full circle', () => {
    const s = sunburstLayout({ id: 'r', label: 'R', value: 4, children: [leaf('x', 4, { children: [leaf('y', 4)] })] }, null, { maxSegments: 1 });
    expect(s.folded?.minAngle).toBe(360);
  });

  it('keeps arcs growing only with a fixed scale', () => {
    const at = (v: number) => sunburstLayout({ id: 'r', label: 'R', value: v, children: [leaf('a', v)] }, null, { scale: 100 }).arcs[0]?.a1 ?? 0;
    expect(at(10)).toBeLessThan(at(20));
    expect(at(20)).toBeCloseTo(72);
    expect(sunburstLayout({ id: 'r', label: 'R', value: 150, children: [leaf('a', 150)] }, null, { scale: 100 }).scale).toBe(150);
  });

  it('keeps ids stable and sorts ties by id', () => {
    const tie: PdSunburstNode = { id: 'r', label: 'R', value: 30, children: [leaf('z', 10), leaf('m', 10), leaf('a', 10)] };
    expect(sunburstLayout(tie, null).arcs.map((a) => a.id)).toEqual(['a', 'm', 'z']);
  });

  it('centres on a node and falls back to the root for an unknown id', () => {
    const s = sunburstLayout(TREE, 'a');
    expect(s.centre.id).toBe('a');
    expect(s.trail.map((n) => n.id)).toEqual(['root', 'a']);
    expect(s.arcs.map((a) => a.id)).toEqual(['a1', 'a2']);
    expect(s.arcs[0]?.a1).toBeCloseTo(225);
    expect(s.arcs[0]?.parentLabel).toBe('A');
    expect(sunburstLayout(TREE, 'nope').centre.id).toBe('root');
  });

  it('limits the rings', () => {
    const deep = (n: number): PdSunburstNode => (n === 0 ? leaf('n0', 1) : { id: `n${n}`, label: `n${n}`, value: 1, children: [deep(n - 1)] });
    expect(sunburstLayout(deep(8), null, { rings: 2 }).arcs.length).toBe(2);
    expect(sunburstLayout(deep(8), null, { rings: 99 }).arcs.length).toBe(6);
    expect(sunburstLayout(deep(8), null, { rings: NaN }).arcs.length).toBe(4);
    expect(sunburstLayout(deep(8), null, { rings: 2 }).arcs[1]?.open).toBe(false);
  });

  it('survives odd input', () => {
    expect(sunburstLayout(leaf('r', NaN), null).arcs).toEqual([]);
    expect(sunburstLayout(leaf('r', 0), null).scale).toBe(0);
    expect(sunburstLayout(leaf('r', 5), null).arcs).toEqual([]);
    const odd: PdSunburstNode = { id: 'r', label: 'R', value: 10, children: [leaf('n', NaN), leaf('neg', -4), leaf('inf', Infinity), leaf('ok', 5), leaf('ok2', 40)] };
    const s = sunburstLayout(odd, null);
    expect(s.arcs.map((a) => a.id)).toEqual(['ok2', 'ok']);
    expect(s.arcs[1]?.a1).toBeCloseTo(360);
    expect(sunburstLayout({ id: 'r', label: 'R', value: 0, children: [leaf('x', 3)] }, null).arcs).toEqual([]);
    const huge: PdSunburstNode = { id: 'r', label: 'R', value: 1e6, children: Array.from({ length: 10000 }, (_, i) => leaf(`f${i}`, 100)) };
    const h = sunburstLayout(huge, null);
    expect(h.arcs.length).toBeLessThanOrEqual(49);
    expect(sunburstLayout({ id: 'r', label: 'R', value: 10, children: [leaf('only', 10)] }, null).arcs.length).toBe(1);
    expect(sunburstLayout({ id: 'r', label: 'R', value: 10, hasMore: true }, null).arcs).toEqual([]);
    expect(sunburstLayout({ id: 'r', label: 'R', value: 10, children: [leaf('h', 10, { hasMore: true })] }, null).arcs[0]?.expandable).toBe(true);
  });

  it('scales children that exceed their parent', () => {
    const s = sunburstLayout({ id: 'r', label: 'R', value: 10, children: [leaf('a', 15), leaf('b', 15)] }, null);
    expect(s.arcs.map((a) => a.a1 - a.a0)).toEqual([180, 180]);
  });
});

describe('geometry', () => {
  it('gives ring radii that shrink outwards and fill the span', () => {
    const r = ringRadii(4, 92, 280);
    expect(r[0]?.r0).toBe(92);
    expect(r[3]?.r1).toBeCloseTo(280);
    expect((r[0]?.r1 ?? 0) - (r[0]?.r0 ?? 0)).toBeGreaterThan((r[3]?.r1 ?? 0) - (r[3]?.r0 ?? 0));
    expect(ringRadii(NaN, 10, 20).length).toBe(4);
    expect(ringRadii(1, 30, 10)[0]).toEqual({ r0: 30, r1: 30 });
  });

  it('draws an annular sector with a gap, a full ring as two circles, and thin slivers without crashing', () => {
    const d = arcPath(100, 100, 40, 80, 0, 90);
    expect(d.startsWith('M ')).toBe(true);
    expect(d).toContain('A 79.1 79.1 0 0 1');
    expect(arcPath(100, 100, 40, 80, 0, 270)).toContain('0 1 1');
    const full = arcPath(100, 100, 40, 80, 0, 360);
    expect(full.match(/M /g)?.length).toBe(2);
    expect(arcPath(100, 100, 40, 41, 10, 10.0001)).toContain('M ');
    expect(arcPath(100, 100, 40, 40.5, 0, 90)).toContain('A 40.5 40.5');
    expect(arcPath(100, 100, 40, 80, 0, 90, -6)).toContain('A 83');
    expect(arcPath(100, 100, 0, 80, 0, 90)).toContain('A 0.9 0.9')
    expect(arcPath(100, 100, 0, 80, 0, 90, -6)).toContain('A 0.5 0.5');
  });

  it('fits labels', () => {
    expect(textWidth('abc')).toBeCloseTo(19.8);
    expect(labelFits(100, 30, 'Xcode')).toBe(true);
    expect(labelFits(40, 30, 'Xcode')).toBe(false);
    expect(labelFits(500, 10, 'Xcode')).toBe(false);
  });

  it('reverses label paths on the lower half so text reads upright', () => {
    const upper = labelPath(100, 100, 50, 10, 60);
    const lower = labelPath(100, 100, 50, 120, 170);
    expect(upper).toContain('0 0 1');
    expect(lower).toContain('0 0 0');
    expect(labelPath(100, 100, 50, -100, 100)).toContain('0 1 1');
  });

  it('computes contrast', () => {
    expect(luminance(255, 255, 255)).toBeCloseTo(1);
    expect(luminance(0, 0, 0)).toBe(0);
    expect(luminance(10, 10, 10)).toBeLessThan(0.01);
    expect(contrastRatio(1, 0)).toBeCloseTo(21);
    expect(contrastRatio(0, 1)).toBeCloseTo(21);
  });
});

describe('nextFocus', () => {
  const arcs = sunburstLayout(TREE, null, { order: ORDER }).arcs;
  it('moves among siblings and stops at the ends', () => {
    expect(nextFocus(arcs, 'a', 'ArrowDown')).toBe('b');
    expect(nextFocus(arcs, 'b', 'ArrowUp')).toBe('a');
    expect(nextFocus(arcs, 'a', 'ArrowUp')).toBeNull();
    expect(nextFocus(arcs, 'pend', 'ArrowDown')).toBeNull();
    expect(nextFocus(arcs, 'b', 'Home')).toBe('a');
    expect(nextFocus(arcs, 'b', 'End')).toBe('pend');
    expect(nextFocus(arcs, 'a', 'Home')).toBeNull();
  });
  it('changes level with Right and Left only', () => {
    expect(nextFocus(arcs, 'a', 'ArrowRight')).toBe('a1');
    expect(nextFocus(arcs, 'c', 'ArrowRight')).toBeNull();
    expect(nextFocus(arcs, 'a1', 'ArrowLeft')).toBe('a');
    expect(nextFocus(arcs, 'a', 'ArrowLeft')).toBeNull();
    expect(nextFocus(arcs, 'a1', 'ArrowDown')).toBe('a2');
    expect(nextFocus(arcs, 'a2', 'ArrowDown')).toBeNull();
  });
  it('types ahead among siblings, wrapping', () => {
    expect(nextFocus(arcs, 'a', 'c')).toBe('c');
    expect(nextFocus(arcs, 'c', 'a')).toBe('a');
    expect(nextFocus(arcs, 'a', 'z')).toBeNull();
    expect(nextFocus(arcs, 'a', 'a')).toBeNull();
    expect(nextFocus(arcs, 'a', ' ')).toBeNull();
    expect(nextFocus(arcs, 'a', 'Backspace')).toBeNull();
    expect(nextFocus(arcs, 'missing', 'End')).toBeNull();
  });
});

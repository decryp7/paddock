import { describe, expect, it } from 'vitest';
import { RING_MAX_SLICES, RING_MIN_DELTA_E, hexToRgb, ringMinDeltaE, ringOrder } from '../../projects/paddock-ui/chart/src/ring';
import { readRules, resolve, type Scheme } from './css';

const rules = readRules();

describe.each(['dark', 'light'] as Scheme[])('donut slice colours under colour-vision deficiency, %s', (scheme) => {
  const v = resolve(rules, scheme, 'debrief');
  const palette = [1, 2, 3, 4, 5, 6, 7, 8].map((i) => v[`--pd-chart-${i}`] ?? '');

  it('has a palette of hex colours the ring can use', () => {
    expect(palette.every((c) => hexToRgb(c) !== null)).toBe(true);
  });

  it.each([2, 3, 4, 5].slice(0, RING_MAX_SLICES - 1))('every neighbouring pair of %i slices, the wrap-around pair included, is ΔE ≥ 20 in normal, protan, deutan and tritan vision', (n) => {
    const order = ringOrder(palette, n);
    expect(new Set(order).size).toBe(n);
    const colours = order.map((i) => hexToRgb(palette[i] ?? '')!);
    for (let i = 0; i < (n === 2 ? 1 : n); i += 1) {
      expect(ringMinDeltaE([colours[i]!, colours[(i + 1) % n]!]), `slots ${order[i]} and ${order[(i + 1) % n]}`).toBeGreaterThanOrEqual(RING_MIN_DELTA_E);
    }
    expect(ringMinDeltaE(colours)).toBeGreaterThanOrEqual(RING_MIN_DELTA_E);
  });

  it('five slices are held to the same rule', () => {
    const order = ringOrder(palette, 5);
    expect(ringMinDeltaE(order.map((i) => hexToRgb(palette[i] ?? '')!))).toBeGreaterThanOrEqual(RING_MIN_DELTA_E);
  });
});

import { describe, expect, it } from 'vitest';
import { RING_MIN_DELTA_E, hexToRgb, ringMinDeltaE } from '../../projects/paddock-ui/chart/src/ring';
import { branchSlots, contrastRatio, luminance } from '../../projects/paddock-ui/chart/src/sunburst-layout';
import { readRules, resolve, type Scheme } from './css';

// `pd-sunburst` colours (docs/plans/pd-sunburst.md section 6): the five branch slots in ring order, in both sibling tones, over both
// palettes: neighbours stay apart under colour-vision deficiency, every fill reads against the surface, and a label ink reaches 4.5:1.

const rules = readRules();
const TONE = 0.12;

type Rgb = readonly [number, number, number];
const towards = (a: Rgb, b: Rgb, t: number): Rgb => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
const lum = (c: Rgb): number => luminance(c[0] * 255, c[1] * 255, c[2] * 255);

describe.each(['dark', 'light'] as Scheme[])('sunburst branch colours, %s', (scheme) => {
  const v = resolve(rules, scheme, 'debrief');
  const palette = [1, 2, 3, 4, 5, 6, 7, 8].map((i) => v[`--pd-chart-${i}`] ?? '');
  const order = branchSlots(palette);
  const base = order.map((i) => hexToRgb(palette[i] ?? '') as Rgb);
  const text = hexToRgb(v['--pd-text'] ?? '') as Rgb;
  const surface = hexToRgb(v['--pd-surface-1'] ?? '') as Rgb;
  const tones = (c: Rgb): Rgb[] => [c, towards(c, text, TONE)];

  it('reads the tokens as hex', () => {
    expect([text, surface, ...base].every((c) => c !== null)).toBe(true);
    expect(order.length).toBe(5);
  });

  it('keeps neighbouring branches (the wrap-around pair included) at ΔE ≥ 20 in all four visions, in both tones', () => {
    for (let i = 0; i < base.length; i += 1) {
      for (const a of tones(base[i] as Rgb)) {
        for (const b of tones(base[(i + 1) % base.length] as Rgb)) {
          expect(ringMinDeltaE([a, b]), `slots ${order[i]} and ${order[(i + 1) % base.length]}`).toBeGreaterThanOrEqual(RING_MIN_DELTA_E);
        }
      }
    }
  });

  it('draws every fill, the neutral one and both tones included, at 3:1 or more on the surface', () => {
    const neutral = hexToRgb(palette[7] ?? '') as Rgb;
    for (const c of [...base, neutral].flatMap(tones)) expect(contrastRatio(lum(c), lum(surface))).toBeGreaterThanOrEqual(3);
  });

  it('has a label ink (the text or the surface token) at 4.5:1 or more on every fill that gets a label', () => {
    const labelled = base.flatMap(tones).filter((c) => Math.max(contrastRatio(lum(c), lum(text)), contrastRatio(lum(c), lum(surface))) >= 4.5);
    // Labels are dropped on a fill without such an ink (the component checks this at run time); most fills must keep one.
    expect(labelled.length).toBeGreaterThanOrEqual(base.length);
  });
});

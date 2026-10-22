import { describe, expect, it } from 'vitest';
import { contrast, deltaE, parseColor, simulate, type Cvd, type Rgba } from './color';
import { ACCENTS, readRules, resolve, type Scheme, type Vars } from './css';

const rules = readRules();
const SCHEMES: Scheme[] = ['dark', 'light'];
const SURFACES = ['--pd-bg', '--pd-surface-1', '--pd-surface-2', '--pd-surface-3'] as const;
const STATUS = ['success', 'warning', 'danger', 'info'] as const;
const CVDS: Cvd[] = ['deuteranopia', 'protanopia', 'tritanopia'];

const col = (v: Vars, name: string): Rgba => {
  const value = v[name];
  if (!value) throw new Error(`token ${name} is missing`);
  return parseColor(value);
};
/** Colour of a (possibly translucent) token as it appears on `surface`. */
const on = (v: Vars, name: string, surface: string): Rgba => {
  const bg = col(v, surface);
  const fg = col(v, name);
  return {
    r: fg.r * fg.a + bg.r * (1 - fg.a),
    g: fg.g * fg.a + bg.g * (1 - fg.a),
    b: fg.b * fg.a + bg.b * (1 - fg.a),
    a: 1,
  };
};
const ratio = (v: Vars, fg: string, bg: string): number => contrast(col(v, fg), on(v, bg, '--pd-surface-1'));

describe.each(SCHEMES)('contrast, %s', (scheme) => {
  const v = resolve(rules, scheme, 'debrief');

  it.each(SURFACES)('text on %s is AAA (7:1)', (s) => {
    expect(contrast(col(v, '--pd-text'), col(v, s))).toBeGreaterThanOrEqual(7);
  });

  it.each(SURFACES)('muted text on %s is AA (4.5:1)', (s) => {
    expect(contrast(col(v, '--pd-text-muted'), col(v, s))).toBeGreaterThanOrEqual(4.5);
  });

  it('muted text on surface-1 and surface-2 is >= 6.5:1 (the guide quotes 7.4 dark, 6.6 light)', () => {
    for (const s of ['--pd-surface-1', '--pd-surface-2']) {
      expect(contrast(col(v, '--pd-text-muted'), col(v, s))).toBeGreaterThanOrEqual(6.5);
    }
  });

  it.each(SURFACES)('subtle text (non-essential) on %s is AA (4.5:1)', (s) => {
    expect(contrast(col(v, '--pd-text-subtle'), col(v, s))).toBeGreaterThanOrEqual(4.5);
  });

  it.each(STATUS)('%s text is AA on every surface and on its own tint', (name) => {
    for (const s of SURFACES) expect(contrast(col(v, `--pd-${name}`), col(v, s))).toBeGreaterThanOrEqual(4.5);
    expect(contrast(col(v, `--pd-${name}`), on(v, `--pd-${name}-tint`, '--pd-surface-1'))).toBeGreaterThanOrEqual(4.5);
  });

  it('neutral status text is AA on surface-1 and its tint', () => {
    expect(ratio(v, '--pd-neutral', '--pd-surface-1')).toBeGreaterThanOrEqual(4.5);
    expect(contrast(col(v, '--pd-neutral'), on(v, '--pd-neutral-tint', '--pd-surface-1'))).toBeGreaterThanOrEqual(4.5);
  });

  it('input and control boundaries are 3:1 against surface-1 and surface-2 (WCAG 1.4.11)', () => {
    expect(ratio(v, '--pd-border-control', '--pd-surface-1')).toBeGreaterThanOrEqual(3);
    expect(contrast(col(v, '--pd-border-control'), col(v, '--pd-surface-2'))).toBeGreaterThanOrEqual(3);
  });

  it('white on the danger fill is AA', () => {
    expect(contrast(col(v, '--pd-on-danger'), col(v, '--pd-danger-fill'))).toBeGreaterThanOrEqual(4.5);
  });

  it.each([1, 2, 3, 4, 5, 6, 7, 8])('chart colour %i is 3:1 against surface-1 and surface-2 (marks)', (i) => {
    expect(ratio(v, `--pd-chart-${i}`, '--pd-surface-1')).toBeGreaterThanOrEqual(3);
    expect(contrast(col(v, `--pd-chart-${i}`), col(v, '--pd-surface-2'))).toBeGreaterThanOrEqual(3);
  });

  it('diverging ends are 3:1 against surface-1', () => {
    for (const n of ['--pd-diverging-neg', '--pd-diverging-pos']) expect(ratio(v, n, '--pd-surface-1')).toBeGreaterThanOrEqual(3);
  });

  it('the chart neutral bar colour is 3:1 against surface-1', () => {
    expect(ratio(v, '--pd-chart-neutral-bar', '--pd-surface-1')).toBeGreaterThanOrEqual(3);
  });

  it('surfaces are told apart: each step differs from the next', () => {
    const l = (n: string): number => contrast(col(v, n), { r: 0, g: 0, b: 0, a: 1 });
    expect(l('--pd-bg')).not.toBeCloseTo(l('--pd-surface-1'), 3);
    expect(l('--pd-surface-1')).not.toBeCloseTo(l('--pd-surface-2'), 3);
  });
});

describe.each(SCHEMES.flatMap((s) => ACCENTS.map((a) => [s, a] as const)))('accent, %s %s', (scheme, accent) => {
  const v = resolve(rules, scheme, accent);

  it('accent text (links, selected icon) is AA on every surface', () => {
    for (const s of SURFACES) expect(contrast(col(v, '--pd-accent'), col(v, s))).toBeGreaterThanOrEqual(4.5);
  });

  it('focus ring (the accent) is 3:1 against every surface (WCAG 2.4.11)', () => {
    for (const s of SURFACES) expect(contrast(col(v, '--pd-focus-ring'), col(v, s))).toBeGreaterThanOrEqual(3);
  });

  it('white text on the accent fill is AA', () => {
    expect(contrast(col(v, '--pd-on-accent'), col(v, '--pd-accent-fill'))).toBeGreaterThanOrEqual(4.5);
  });

  it('the accent fill (button) is 3:1 against surface-1', () => {
    expect(contrast(col(v, '--pd-accent-fill'), col(v, '--pd-surface-1'))).toBeGreaterThanOrEqual(3);
  });
});

describe('accent hue is not a status hue', () => {
  const hue = (c: Rgba): number => {
    const [r, g, b] = [c.r / 255, c.g / 255, c.b / 255];
    const max = Math.max(r, g, b);
    const d = max - Math.min(r, g, b);
    if (d === 0) return 0;
    const h = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
    return (h * 60 + 360) % 360;
  };
  const dist = (a: number, b: number): number => Math.min(Math.abs(a - b), 360 - Math.abs(a - b));
  it.each(ACCENTS)('%s fill stays 20 degrees away from the warning hue', (accent) => {
    const v = resolve(rules, 'dark', accent);
    expect(dist(hue(col(v, '--pd-accent-fill')), hue(col(v, '--pd-warning')))).toBeGreaterThanOrEqual(20);
  });
});

describe.each(SCHEMES)('chart palette under colour-vision deficiency, %s', (scheme) => {
  const v = resolve(rules, scheme, 'debrief');
  const palette = [1, 2, 3, 4, 5, 6, 7, 8].map((i) => col(v, `--pd-chart-${i}`));
  const SURFACE_1 = col(v, '--pd-surface-1');

  it.each(CVDS)('adjacent series stay apart under %s (CIE76 dE >= 20)', (kind) => {
    for (let i = 0; i + 1 < palette.length; i++) {
      const d = deltaE(simulate(palette[i] as Rgba, kind), simulate(palette[i + 1] as Rgba, kind));
      expect(d, `series ${i + 1} vs ${i + 2} under ${kind}`).toBeGreaterThanOrEqual(20);
    }
  });

  it.each(CVDS)("the first five (recommended) series are all pairwise >= 14 apart under %s", (kind) => {
    const sim = palette.slice(0, 5).map((c) => simulate(c, kind));
    for (let i = 0; i < sim.length; i++)
      for (let j = i + 1; j < sim.length; j++) {
        expect(deltaE(sim[i] as Rgba, sim[j] as Rgba), `${i + 1} vs ${j + 1} under ${kind}`).toBeGreaterThanOrEqual(14);
      }
  });

  it('normal vision: every pair of the 8 colours is >= 20 apart', () => {
    for (let i = 0; i < 8; i++)
      for (let j = i + 1; j < 8; j++) expect(deltaE(palette[i] as Rgba, palette[j] as Rgba)).toBeGreaterThanOrEqual(20);
  });

  it('the diverging ends are far apart under every deficiency (never red/green)', () => {
    for (const kind of CVDS) {
      const d = deltaE(simulate(col(v, '--pd-diverging-neg'), kind), simulate(col(v, '--pd-diverging-pos'), kind));
      expect(d, kind).toBeGreaterThanOrEqual(30);
    }
  });

  it('status hues without their icons: success vs danger stay apart under red-green deficiency (dE >= 10; icon and word carry the meaning)', () => {
    for (const kind of ['deuteranopia', 'protanopia'] as Cvd[]) {
      const d = deltaE(simulate(col(v, '--pd-success'), kind), simulate(col(v, '--pd-danger'), kind));
      expect(d, kind).toBeGreaterThanOrEqual(10);
    }
  });

  it('simulation leaves greys unchanged', () => {
    const g = simulate({ r: 128, g: 128, b: 128, a: 1 }, 'deuteranopia');
    expect(Math.abs(g.r - 128)).toBeLessThan(3);
    expect(SURFACE_1.a).toBe(1);
  });
});

describe('density and structure', () => {
  it.each([
    ['comfortable', '36px', '20px'],
    ['compact', '24px', '12px'],
  ])('%s sets row height %s and card padding %s', (name, row, pad) => {
    const r = rules.get(`:root[data-density=${name}]`);
    expect(r?.['--pd-row-height']).toBe(row);
    expect(r?.['--pd-card-padding']).toBe(pad);
  });

  it('standard is the default (32 / 16)', () => {
    expect(rules.get(':root')?.['--pd-row-height']).toBe('32px');
    expect(rules.get(':root')?.['--pd-card-padding']).toBe('16px');
  });

  it('hit targets are at least 24 px', () => {
    expect(rules.get(':root')?.['--pd-hit-min']).toBe('24px');
  });
});

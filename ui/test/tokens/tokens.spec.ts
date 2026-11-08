import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { contrast, deltaE, deltaE2000, lab, parseColor, simulate, type Cvd, type Rgba } from './color';
import { ACCENTS, readRules, resolve, type Contrast, type Scheme, type Vars } from './css';

const rules = readRules();
const SCHEMES: Scheme[] = ['dark', 'light'];
const CONTRASTS: Contrast[] = ['standard', 'more'];
const MODES = SCHEMES.flatMap((s) => CONTRASTS.map((c) => [s, c] as const));
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

describe.each(MODES)('contrast, %s, %s contrast', (scheme, mode) => {
  const v = resolve(rules, scheme, 'debrief', mode);

  it('the resting "..." button of a table row is 3:1 on a plain row, and the full subtle text is 3:1 on the selected tint and on surface-2', () => {
    const mix = (a: Rgba, b: Rgba, k: number): Rgba => ({ r: a.r * k + b.r * (1 - k), g: a.g * k + b.g * (1 - k), b: a.b * k + b.b * (1 - k), a: 1 });
    const [subtle, s1, s2, accent] = [col(v, '--pd-text-subtle'), col(v, '--pd-surface-1'), col(v, '--pd-surface-2'), col(v, '--pd-accent')];
    // --pd-row-action-rest: subtle at 75 % mixed into surface-1; a selected row (the accent tint, about 14 %) shows the full subtle text instead.
    const rest = mix(subtle, s1, 0.75);
    expect(contrast(rest, s1)).toBeGreaterThanOrEqual(3);
    // Rows that sit on surface-2 (the group band, hover) show the button in the text colour, never in the rest mix.
    expect(contrast(rest, s2), 'rest on surface-2 (information only)').toBeGreaterThan(2.5);
    const selected = mix(accent, s1, 0.14);
    expect(contrast(subtle, selected), 'full subtle on the selected tint').toBeGreaterThanOrEqual(3);
    expect(contrast(subtle, s2), 'full subtle on surface-2').toBeGreaterThanOrEqual(3);
    expect(v['--pd-row-action-rest'] ?? '').toContain('75%');
  });

  it('the code surface differs from the pane and keeps text at 4.5:1 or better', () => {
    expect(col(v, '--pd-surface-code')).not.toEqual(col(v, '--pd-surface-1'));
    expect(contrast(col(v, '--pd-text'), col(v, '--pd-surface-code'))).toBeGreaterThanOrEqual(7);
    expect(contrast(col(v, '--pd-text-muted'), col(v, '--pd-surface-code'))).toBeGreaterThanOrEqual(4.5);
  });

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

  it('the widget grid\'s grips, snapping guides and placeholder are 3:1 (WCAG 1.4.11, ADR 0044)', () => {
    // Grips: --pd-text-muted on the widget surface.
    expect(contrast(col(v, '--pd-text-muted'), col(v, '--pd-surface-1'))).toBeGreaterThanOrEqual(3);
    expect(contrast(col(v, '--pd-text-muted'), col(v, '--pd-surface-2'))).toBeGreaterThanOrEqual(3);
    // Snapping guides (--pd-snap-guide: the accent at 70 %) meet the window background, behind the widgets.
    expect(v['--pd-snap-guide'] ?? '').toContain('70%');
    const a = col(v, '--pd-accent');
    const bg = col(v, '--pd-bg');
    const guide: Rgba = { r: a.r * 0.7 + bg.r * 0.3, g: a.g * 0.7 + bg.g * 0.3, b: a.b * 0.7 + bg.b * 0.3, a: 1 };
    expect(contrast(guide, bg)).toBeGreaterThanOrEqual(3);
    // The placeholder's dashed outline (the accent) on the background and on its own tint.
    expect(contrast(a, bg)).toBeGreaterThanOrEqual(3);
    const tint: Rgba = { r: a.r * 0.14 + bg.r * 0.86, g: a.g * 0.14 + bg.g * 0.86, b: a.b * 0.14 + bg.b * 0.86, a: 1 };
    expect(contrast(a, tint)).toBeGreaterThanOrEqual(3);
    // The focus ring on the widget surface.
    expect(contrast(col(v, '--pd-focus-ring'), col(v, '--pd-surface-1'))).toBeGreaterThanOrEqual(3);
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

  it('the meter neutral fill is 3:1 on every surface a meter sits on (ADR 0035)', () => {
    for (const s of ['--pd-surface-1', '--pd-surface-2', '--pd-surface-3']) expect(contrast(col(v, '--pd-meter-neutral'), col(v, s))).toBeGreaterThanOrEqual(3);
  });

  it('the meter neutral fill is 3:1 against its track (the chart grid over surface-2 and surface-3)', () => {
    for (const s of ['--pd-surface-2', '--pd-surface-3']) expect(contrast(col(v, '--pd-meter-neutral'), on(v, '--pd-chart-grid', s))).toBeGreaterThanOrEqual(3);
  });

  it('the raised skeleton shows on a surface-2 fill and its shine is a step apart (ADR 0035)', () => {
    expect(contrast(col(v, '--pd-skeleton-raised'), col(v, '--pd-surface-2'))).toBeGreaterThanOrEqual(1.2);
    expect(contrast(col(v, '--pd-skeleton-raised'), col(v, '--pd-skeleton-raised-shine'))).toBeGreaterThan(1.05);
  });

  it('surfaces are told apart: each step differs from the next', () => {
    const l = (n: string): number => contrast(col(v, n), { r: 0, g: 0, b: 0, a: 1 });
    expect(l('--pd-bg')).not.toBeCloseTo(l('--pd-surface-1'), 3);
    expect(l('--pd-surface-1')).not.toBeCloseTo(l('--pd-surface-2'), 3);
  });
});

describe.each(MODES.flatMap(([s, c]) => ACCENTS.map((a) => [s, c, a] as const)))('accent, %s, %s contrast, %s', (scheme, mode, accent) => {
  const v = resolve(rules, scheme, accent, mode);

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

  // Slot 6 is the one the review found too close to slot 1 (both blues); it keeps clear of every other slot. Other weak pairs exist
  // in any 8-colour palette (1 and 4, 2 and 5): that is why a chart with 5 or more series must also be keyed without colour.
  it.each(['protanopia', 'deuteranopia'] as Cvd[])('slot 6 is >= 20 apart from every other slot under %s', (kind) => {
    const sim = palette.map((c) => simulate(c, kind));
    for (let i = 0; i < sim.length; i++) {
      if (i === 5) continue;
      expect(deltaE(sim[i] as Rgba, sim[5] as Rgba), `${i + 1} vs 6 under ${kind}`).toBeGreaterThanOrEqual(20);
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

describe('seven app accents and the demos\' graphite (one per app, ADR 0012 §5)', () => {
  it('every app of the family has an accent, Pacenotes and Sighter included, and the demo apps have graphite', () => {
    expect([...ACCENTS].sort()).toEqual(['debrief', 'dyno', 'graphite', 'pacenotes', 'pitcrew', 'pitwall', 'sighter', 'spotter']);
  });

  it.each(ACCENTS)('%s has its own rule for each scheme', (accent) => {
    for (const scheme of SCHEMES) expect(rules.get(`:root[data-theme=${scheme}][data-accent=${accent}]`)?.['--pd-accent']).toBeTruthy();
  });

  it('accent fills are told apart from each other (normal vision dE >= 15)', () => {
    const fills = ACCENTS.map((a) => [a, col(resolve(rules, 'dark', a), '--pd-accent-fill')] as const);
    for (let i = 0; i < fills.length; i++)
      for (let j = i + 1; j < fills.length; j++) {
        const [a, x] = fills[i] as (typeof fills)[number];
        const [b, y] = fills[j] as (typeof fills)[number];
        expect(deltaE(x, y), `${a} vs ${b}`).toBeGreaterThanOrEqual(15);
      }
  });
});

describe('Pacenotes, Sighter and graphite accents (chosen 2026-10-24 by CIEDE2000, design guide §4)', () => {
  const text = (accent: (typeof ACCENTS)[number], scheme: Scheme): Rgba => col(resolve(rules, scheme, accent), '--pd-accent');
  const status = (name: string, scheme: Scheme): Rgba => col(resolve(rules, scheme, 'debrief'), `--pd-${name}`);
  const known = { reference: deltaE2000(parseColor('#50a0a0'), parseColor('#50a0a0')), sharma: deltaE2000({ r: 0, g: 0, b: 0, a: 1 }, { r: 255, g: 255, b: 255, a: 1 }) };

  it('CIEDE2000 is the published formula (identical colours 0, black to white 100)', () => {
    expect(known.reference).toBe(0);
    expect(known.sharma).toBeCloseTo(100, 0);
  });

  it.each(['pacenotes', 'sighter', 'graphite'] as const)('%s never reads as a status: CIEDE2000 >= 15 from success, warning, danger and info (normal vision)', (accent) => {
    for (const scheme of SCHEMES)
      for (const name of STATUS) expect(deltaE2000(text(accent, scheme), status(name, scheme)), `${scheme} ${name}`).toBeGreaterThanOrEqual(15);
  });

  it.each(['pacenotes', 'sighter', 'graphite'] as const)('%s is its own colour in the family: CIEDE2000 >= 12 from every other accent (normal vision)', (accent) => {
    for (const scheme of SCHEMES)
      for (const other of ACCENTS.filter((a) => a !== accent))
        expect(deltaE2000(text(accent, scheme), text(other, scheme)), `${scheme} ${other}`).toBeGreaterThanOrEqual(12);
  });
});

describe.each(SCHEMES)('high contrast, %s', (scheme) => {
  const v = resolve(rules, scheme, 'debrief', 'more');
  const standard = resolve(rules, scheme, 'debrief');

  it('hairlines become boundaries of 3:1 against surface-1 (they are translucent and faint otherwise)', () => {
    expect(ratio(standard, '--pd-hairline', '--pd-surface-1')).toBeLessThan(3);
    expect(ratio(v, '--pd-hairline', '--pd-surface-1')).toBeGreaterThanOrEqual(3);
  });

  it('removes translucency: opaque hairlines, popovers on surface-1, no soft shadows', () => {
    expect(col(v, '--pd-hairline').a).toBe(1);
    expect(v['--pd-surface-3']).toBe(v['--pd-surface-1']);
    expect(v['--pd-shadow-1']).toBe('none');
    expect(v['--pd-shadow-2']).toContain('var(--pd-border-control)');
  });

  it('subtle text is as strong as muted text (>= 6.5:1 on surface-1)', () => {
    expect(contrast(col(v, '--pd-text-subtle'), col(v, '--pd-surface-1'))).toBeGreaterThanOrEqual(6.5);
  });

  it('the scrim is darker than in standard contrast', () => {
    expect(col(v, '--pd-scrim').a).toBeGreaterThan(col(standard, '--pd-scrim').a);
  });

  it('the strong border is a 3:1 boundary on the fills a widget frame sits on (the frame outline, ADR 0035)', () => {
    for (const surface of ['--pd-surface-2', '--pd-surface-3']) expect(contrast(on(v, '--pd-border-strong', surface), col(v, surface)), surface).toBeGreaterThanOrEqual(3);
  });

  it('applies by the OS preference too, unless the app turned it off', () => {
    const os = rules.get('@media (prefers-contrast: more) | :root:not([data-contrast=off])');
    expect(os?.['--pd-hairline']).toBe(rules.get(':root[data-contrast=more]')?.['--pd-hairline']);
  });
});

describe('reduced motion', () => {
  const reduced = rules.get(':root[data-motion=reduce]');
  const ms = (v: string | undefined, all: Vars): number => {
    const ref = /^var\((--[\w-]+)\)$/.exec(v ?? '');
    return Number.parseFloat((ref ? all[ref[1] as string] : v) ?? 'NaN');
  };

  it('every duration is at most 100 ms (fades only) and translation is off', () => {
    const all = { ...rules.get(':root'), ...reduced };
    for (const name of ['--pd-duration-micro', '--pd-duration', '--pd-duration-large']) expect(ms(all[name], all), name).toBeLessThanOrEqual(100);
    expect(all['--pd-motion-scale']).toBe('0');
  });

  it('full motion by default', () => {
    expect(rules.get(':root')?.['--pd-motion-scale']).toBe('1');
    expect(Number.parseFloat(rules.get(':root')?.['--pd-duration-large'] ?? '')).toBeGreaterThan(100);
  });

  it('follows the OS setting unless the app chose full motion', () => {
    expect(rules.get('@media (prefers-reduced-motion: reduce) | :root:not([data-motion=full])')).toEqual(reduced);
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

describe('widget frame (ADR 0035)', () => {
  const src = (f: string): string => readFileSync(new URL(`../../projects/paddock-ui/widgets/src/${f}`, import.meta.url), 'utf8');

  it('is outlined with the strong border colour in high contrast, by the OS preference and by the app setting', () => {
    for (const f of ['frame.ts', 'area.ts']) {
      const text = src(f);
      expect(text, f).toMatch(/@media \(prefers-contrast: more\) \{ :root:not\(\[data-contrast='off'\]\)[^}]*outline: 1px solid var\(--pd-border-strong\)/);
      expect(text, f).toMatch(/:root\[data-contrast='more'\][^}]*outline: 1px solid var\(--pd-border-strong\)/);
    }
  });

  it('keeps one geometry at every density: no row-height or card-padding token, the popup is 8 / 12 px by design', () => {
    for (const f of ['frame.ts', 'area.ts']) expect(src(f), f).not.toMatch(/--pd-(row-height|card-padding)/);
    expect(src('frame.ts')).toContain('padding: var(--pd-space-2) var(--pd-space-3)');
  });
});

// The count ramp (--pd-count-1 to -5, fewest to most): five legend steps drawn as country fills on Dyno's connections map, where
// neighbouring countries are often neighbouring steps. Lightness carries the order (it survives every colour-vision deficiency):
// neighbours are CIEDE2000 >= 10 apart and at least 1.3:1 in luminance (1.4:1 for the last pair, the darkest or lightest end) in
// normal vision and under protanopia, deuteranopia and tritanopia, and L* >= 10 apart, rising with the count in dark and falling
// in light, with the dark end at L* >= 12 so it never reads as text. Step 1 is 3:1 on the page in normal vision only (WCAG 1.4.11), and
// every step stays clear of the warning colour of the hatched "flagged" marker (the hatch and the flag glyph carry it too). The
// map's markers are the text colour with a page-colour halo, outside the ramp: CIEDE2000 >= 15 in normal vision; fill or halo 3:1
// in every vision.
const COUNT_MIN_DE00 = 10;
const COUNT_MIN_DL = 10;
const COUNT_MIN_RATIO = 1.3;
const COUNT_MIN_RATIO_END = 1.4;
const COUNT_MIN_END_L = 12;
const COUNT_MIN_DE00_WARNING = 7;
const COUNT_MIN_DE00_MARKER = 15;
const RAMPED = ACCENTS.filter((a) => resolve(rules, 'dark', a)['--pd-count-1'] !== undefined);

describe('count ramp', () => {
  it('Dyno defines one (its connections map uses it)', () => {
    expect(RAMPED).toContain('dyno');
  });
});

describe.each(SCHEMES.flatMap((s) => RAMPED.map((a) => [s, a] as const)))('count ramp, %s, %s', (scheme, accent) => {
  const v = resolve(rules, scheme, accent);
  const ramp = [1, 2, 3, 4, 5].map((i) => col(v, `--pd-count-${i}`));
  const L = (c: Rgba): number => lab(c)[0];

  it.each(['normal', ...CVDS] as const)(`neighbouring steps are CIEDE2000 >= ${COUNT_MIN_DE00} apart, %s`, (kind) => {
    const see = (c: Rgba): Rgba => (kind === 'normal' ? c : simulate(c, kind));
    for (let i = 0; i + 1 < ramp.length; i++) {
      expect(deltaE2000(see(ramp[i] as Rgba), see(ramp[i + 1] as Rgba)), `step ${i + 1} vs ${i + 2}`).toBeGreaterThanOrEqual(COUNT_MIN_DE00);
    }
  });

  it(`lightness orders the steps: L* >= ${COUNT_MIN_DL} per step, lighter with the count in dark, darker in light`, () => {
    for (let i = 0; i + 1 < ramp.length; i++) {
      const step = (L(ramp[i + 1] as Rgba) - L(ramp[i] as Rgba)) * (scheme === 'dark' ? 1 : -1);
      expect(step, `step ${i + 1} to ${i + 2}`).toBeGreaterThanOrEqual(COUNT_MIN_DL);
    }
  });

  it.each(['normal', ...CVDS] as const)(`neighbouring steps are ${COUNT_MIN_RATIO}:1 apart in luminance (the last pair ${COUNT_MIN_RATIO_END}:1), %s`, (kind) => {
    const see = (c: Rgba): Rgba => (kind === 'normal' ? c : simulate(c, kind));
    for (let i = 0; i + 1 < ramp.length; i++) {
      const min = i + 2 === ramp.length ? COUNT_MIN_RATIO_END : COUNT_MIN_RATIO;
      expect(contrast(see(ramp[i] as Rgba), see(ramp[i + 1] as Rgba)), `step ${i + 1} vs ${i + 2}`).toBeGreaterThanOrEqual(min);
    }
  });

  it(`the darkest step is L* >= ${COUNT_MIN_END_L} (it never reads as black ink)`, () => {
    expect(Math.min(...ramp.map(L))).toBeGreaterThanOrEqual(COUNT_MIN_END_L);
  });

  it(`the marker (the text colour) is CIEDE2000 >= ${COUNT_MIN_DE00_MARKER} from every step, normal vision`, () => {
    for (const [i, c] of ramp.entries()) expect(deltaE2000(c, col(v, '--pd-text')), `step ${i + 1}`).toBeGreaterThanOrEqual(COUNT_MIN_DE00_MARKER);
  });

  it.each(['normal', ...CVDS] as const)('the marker\'s fill (the text colour) or its halo (the page colour) is 3:1 against every step, %s', (kind) => {
    const see = (c: Rgba): Rgba => (kind === 'normal' ? c : simulate(c, kind));
    for (const [i, c] of ramp.entries()) {
      const best = Math.max(contrast(see(col(v, '--pd-text')), see(c)), contrast(see(col(v, '--pd-bg')), see(c)));
      expect(best, `step ${i + 1}`).toBeGreaterThanOrEqual(3);
    }
  });

  it('every step is 3:1 against the page (the map\'s sea), normal vision', () => {
    for (const [i, c] of ramp.entries()) expect(contrast(c, col(v, '--pd-bg')), `step ${i + 1}`).toBeGreaterThanOrEqual(3);
  });

  it.each(['normal', ...CVDS] as const)(`every step is CIEDE2000 >= ${COUNT_MIN_DE00_WARNING} from the warning colour (the flagged hatch), %s`, (kind) => {
    const see = (c: Rgba): Rgba => (kind === 'normal' ? c : simulate(c, kind));
    for (const [i, c] of ramp.entries()) expect(deltaE2000(see(c), see(col(v, '--pd-warning'))), `step ${i + 1}`).toBeGreaterThanOrEqual(COUNT_MIN_DE00_WARNING);
  });
});

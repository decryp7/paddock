import { SU_ACCENTS, SU_CONTRASTS, SU_DENSITIES, SU_MOTIONS, SU_THEMES, applyPaddockAppearance } from './appearance';
import {
  EVENT_SHAPES,
  PADDOCK_THEME_DARK,
  PADDOCK_THEME_LIGHT,
  areaGradient,
  createEchartsTheme,
  eventLaneFragment,
  registerPaddockTheme,
} from './echarts/theme';
import { type PaddockChartTokens, mixColors, parseCssColor, readPaddockTokens, sequentialRamp, withAlpha } from './echarts/tokens';

const tokens = (scheme: 'dark' | 'light' = 'dark'): PaddockChartTokens => ({
  scheme,
  fontFamily: 'system-ui',
  text: '#eceef1',
  textMuted: '#a0a7b2',
  textSubtle: '#8b93a0',
  surface1: '#15171b',
  surface2: '#1b1e23',
  surface3: '#22262c',
  hairline: 'rgba(255, 255, 255, 0.08)',
  borderStrong: 'rgba(255, 255, 255, 0.16)',
  accent: '#5b9bff',
  palette: ['#60aeeb', '#f3aa25', '#3dd39d', '#d68cc2', '#f07d3a', '#8fa3ff', '#e8dc5a', '#a2acb1'],
  divergingNeg: '#60aeeb',
  divergingMid: '#22262c',
  divergingPos: '#f3aa25',
  neutralBar: '#5e6775',
  success: '#3dd68c',
  warning: '#f2b84b',
  danger: '#ff6b6b',
  info: '#7db2e8',
});

describe('colour helpers', () => {
  it('parses hex, short hex and rgb(a) in both syntaxes', () => {
    expect(parseCssColor('#ff8000')).toEqual({ r: 255, g: 128, b: 0, a: 1 });
    expect(parseCssColor('#f80')).toEqual({ r: 255, g: 136, b: 0, a: 1 });
    expect(parseCssColor('rgb(1 2 3 / 0.5)')).toEqual({ r: 1, g: 2, b: 3, a: 0.5 });
    expect(parseCssColor('rgba(1, 2, 3, 0.25)')).toEqual({ r: 1, g: 2, b: 3, a: 0.25 });
    expect(parseCssColor('rgb(1 2 3 / 50%)')?.a).toBe(0.5);
    expect(parseCssColor('rgb(1 2 3)')?.a).toBe(1);
  });
  it('rejects what it cannot parse', () => {
    expect(parseCssColor('color-mix(in srgb, red, blue)')).toBeNull();
    expect(parseCssColor('')).toBeNull();
  });
  it('withAlpha and mixColors fall back to their input on unparseable colours', () => {
    expect(withAlpha('#ff0000', 0.5)).toBe('rgba(255, 0, 0, 0.5)');
    expect(withAlpha('red', 0.5)).toBe('red');
    expect(mixColors('#000000', '#ffffff', 0.5)).toBe('#808080');
    expect(mixColors('red', '#ffffff', 0.5)).toBe('#ffffff');
    expect(mixColors('#000000', 'blue', 0.5)).toBe('blue');
  });
  it('sequentialRamp goes from surface-2 to the accent in the asked number of steps', () => {
    const r = sequentialRamp({ surface2: '#1b1e23', accent: '#5b9bff' });
    expect(r).toHaveLength(6);
    expect(r[0]).toBe('#1b1e23');
    expect(r[5]).toBe('#5b9bff');
    expect(sequentialRamp({ surface2: '#000000', accent: '#ffffff' }, 3)).toEqual(['#000000', '#808080', '#ffffff']);
  });
});

describe('readPaddockTokens', () => {
  afterEach(() => {
    document.documentElement.removeAttribute('style');
  });
  it('reads custom properties from the element, with a font fallback and the 8-colour palette', () => {
    const el = document.documentElement;
    el.style.setProperty('--pd-text', '#111111');
    el.style.setProperty('--pd-chart-3', '#00ff00');
    el.style.colorScheme = 'light';
    const t = readPaddockTokens(el);
    expect(t.text).toBe('#111111');
    expect(t.palette).toHaveLength(8);
    expect(t.palette[2]).toBe('#00ff00');
    expect(t.fontFamily).toBe('system-ui, sans-serif');
  });
  it('defaults to dark and recognises light', () => {
    const el = document.createElement('div');
    document.body.appendChild(el);
    expect(readPaddockTokens(el).scheme).toBe('dark');
    el.style.colorScheme = 'light';
    expect(readPaddockTokens(el).scheme).toBe('light');
    el.remove();
  });
});

describe('createEchartsTheme', () => {
  const theme = createEchartsTheme(tokens()) as Record<string, any>;
  it('uses the Paddock palette in stable order and a transparent background', () => {
    expect(theme['color']).toEqual(tokens().palette);
    expect(theme['backgroundColor']).toBe('transparent');
    expect(theme['darkMode']).toBe(true);
    expect((createEchartsTheme(tokens('light')) as Record<string, any>)['darkMode']).toBe(false);
  });
  it('has no axis lines except the category/time baseline; value grid is dashed hairline', () => {
    expect(theme['valueAxis'].axisLine.show).toBe(false);
    expect(theme['categoryAxis'].axisLine.show).toBe(true);
    expect(theme['timeAxis'].axisLine.show).toBe(true);
    expect(theme['valueAxis'].splitLine.show).toBe(true);
    expect(theme['valueAxis'].splitLine.lineStyle.type).toEqual([2, 4]);
    expect(theme['categoryAxis'].splitLine.show).toBe(false);
    expect(theme['valueAxis'].axisLabel.fontSize).toBe(11);
  });
  it('lines are 1.5 px, symbol-free, gap-honest and sampled; live updates do not animate', () => {
    expect(theme['line'].lineStyle.width).toBe(1.5);
    expect(theme['line'].showSymbol).toBe(false);
    expect(theme['line'].connectNulls).toBe(false);
    expect(theme['line'].sampling).toBe('lttb');
    expect(theme['animationDuration']).toBe(300);
    expect(theme['animationDurationUpdate']).toBe(0);
  });
  it('bars have a 4 px top radius and a 25% gap; legend is top-left; tooltip is surface-3 with a crosshair', () => {
    expect(theme['bar'].itemStyle.borderRadius).toEqual([4, 4, 0, 0]);
    expect(theme['bar'].barCategoryGap).toBe('25%');
    expect(theme['legend'].left).toBe(0);
    expect(theme['tooltip'].backgroundColor).toBe('#22262c');
    expect(theme['tooltip'].axisPointer.type).toBe('cross');
  });
  it('has no slider chrome by default', () => {
    expect(theme['dataZoom'].show).toBe(false);
  });
});

describe('areaGradient', () => {
  it('fades from 28% alpha at the top to 0 at the baseline', () => {
    const g = areaGradient('#ff0000');
    expect(g.colorStops).toEqual([
      { offset: 0, color: 'rgba(255, 0, 0, 0.28)' },
      { offset: 1, color: 'rgba(255, 0, 0, 0)' },
    ]);
    expect(g.y2).toBe(1);
    expect(areaGradient('#ff0000', 0.1).colorStops[0]?.color).toContain('0.1');
  });
});

describe('eventLaneFragment', () => {
  const events = [
    { at: 1000, kind: 'throttle', label: 'Throttled' },
    { at: 2000, kind: 'mystery' },
  ];
  it('puts events on a 14 px lane with shape-coded symbols and a reserved plot margin', () => {
    const f = eventLaneFragment(events, tokens(), 1, { shapes: { throttle: 'triangle' } });
    expect(f.grid.height).toBe(14);
    expect(f.plotBottom).toBe(8 + 14 + 8);
    expect(f.xAxis.gridIndex).toBe(1);
    expect(f.series.data[0]).toMatchObject({ name: 'Throttled', symbol: 'triangle', value: [1000, 0.5] });
    expect(f.series.data[1]).toMatchObject({ name: 'mystery', symbol: 'circle' });
    expect(f.series.tooltip.formatter({ name: 'n' })).toBe('n');
  });
  it('honours lane height and bottom overrides', () => {
    const f = eventLaneFragment([], tokens(), 2, { shapes: {}, laneHeight: 20, bottom: 4 });
    expect(f.grid.height).toBe(20);
    expect(f.grid.bottom).toBe(4);
    expect(f.plotBottom).toBe(4 + 20 + 8);
  });
  it('knows the four shapes', () => {
    expect(EVENT_SHAPES).toEqual(['circle', 'triangle', 'diamond', 'rect']);
  });
});

describe('registerPaddockTheme', () => {
  it('registers under the scheme name and returns it', () => {
    const calls: [string, object][] = [];
    const registry = { registerTheme: (n: string, t: object) => void calls.push([n, t]) };
    expect(registerPaddockTheme(registry, tokens('dark'))).toBe(PADDOCK_THEME_DARK);
    expect(registerPaddockTheme(registry, tokens('light'))).toBe(PADDOCK_THEME_LIGHT);
    expect(calls.map((c) => c[0])).toEqual(['paddock-dark', 'paddock-light']);
  });
});

describe('applyPaddockAppearance', () => {
  it('sets theme, accent and density attributes', () => {
    const el = document.createElement('html');
    applyPaddockAppearance(el, { theme: 'light', accent: 'dyno', density: 'compact' });
    expect(el.getAttribute('data-theme')).toBe('light');
    expect(el.getAttribute('data-accent')).toBe('dyno');
    expect(el.getAttribute('data-density')).toBe('compact');
  });
  it('system removes data-theme so the OS decides', () => {
    const el = document.createElement('html');
    applyPaddockAppearance(el, { theme: 'dark' });
    applyPaddockAppearance(el, { theme: 'system' });
    expect(el.hasAttribute('data-theme')).toBe(false);
  });
  it('leaves untouched what is not passed and ignores unknown values', () => {
    const el = document.createElement('html');
    el.setAttribute('data-accent', 'pitcrew');
    applyPaddockAppearance(el, {});
    applyPaddockAppearance(el, { theme: 'neon' as never, accent: 'nope' as never, density: 'huge' as never });
    expect(el.getAttribute('data-accent')).toBe('pitcrew');
    expect(el.hasAttribute('data-theme')).toBe(false);
    expect(el.hasAttribute('data-density')).toBe(false);
  });
  it('lists the seven apps, three themes and three densities', () => {
    expect(SU_ACCENTS).toEqual(['debrief', 'pitcrew', 'pitwall', 'dyno', 'spotter', 'pacenotes', 'sighter']);
    expect(SU_THEMES).toEqual(['system', 'dark', 'light']);
    expect(SU_DENSITIES).toEqual(['comfortable', 'standard', 'compact']);
    expect(SU_CONTRASTS).toEqual(['system', 'standard', 'more']);
    expect(SU_MOTIONS).toEqual(['system', 'reduce', 'full']);
  });
  it('high contrast: more sets data-contrast, standard turns the OS preference off, system removes it', () => {
    const el = document.createElement('html');
    applyPaddockAppearance(el, { contrast: 'more' });
    expect(el.getAttribute('data-contrast')).toBe('more');
    applyPaddockAppearance(el, { contrast: 'standard' });
    expect(el.getAttribute('data-contrast')).toBe('off');
    applyPaddockAppearance(el, { contrast: 'system' });
    expect(el.hasAttribute('data-contrast')).toBe(false);
  });
  it('motion: reduce and full set data-motion, system removes it, unknown values are ignored', () => {
    const el = document.createElement('html');
    applyPaddockAppearance(el, { motion: 'reduce' });
    expect(el.getAttribute('data-motion')).toBe('reduce');
    applyPaddockAppearance(el, { motion: 'fast' as never, contrast: 'max' as never });
    expect(el.getAttribute('data-motion')).toBe('reduce');
    expect(el.hasAttribute('data-contrast')).toBe(false);
    applyPaddockAppearance(el, { motion: 'full' });
    expect(el.getAttribute('data-motion')).toBe('full');
    applyPaddockAppearance(el, { motion: 'system' });
    expect(el.hasAttribute('data-motion')).toBe(false);
  });
});

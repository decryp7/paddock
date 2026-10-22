/** The resolved token values the chart theme is built from (plain colour strings ECharts can parse). */
export interface SuiteChartTokens {
  scheme: 'dark' | 'light';
  fontFamily: string;
  text: string;
  textMuted: string;
  textSubtle: string;
  surface1: string;
  surface2: string;
  surface3: string;
  hairline: string;
  borderStrong: string;
  accent: string;
  /** The 8 categorical series colours, in stable order. */
  palette: string[];
  divergingNeg: string;
  divergingMid: string;
  divergingPos: string;
  neutralBar: string;
  success: string;
  warning: string;
  danger: string;
  info: string;
}

const PALETTE_SIZE = 8;

function read(style: CSSStyleDeclaration, name: string, fallback = ''): string {
  return style.getPropertyValue(name).trim() || fallback;
}

/**
 * Reads the chart tokens from the computed style of `el` (default: the document element). Custom properties that
 * reference others are resolved by the browser. Colours defined with color-mix() are not used here: the sequential
 * ramp is computed by {@link sequentialRamp}.
 */
export function readSuiteTokens(el: Element = document.documentElement): SuiteChartTokens {
  const style = getComputedStyle(el);
  const scheme = style.colorScheme.includes('light') && !style.colorScheme.includes('dark') ? 'light' : 'dark';
  return {
    scheme,
    fontFamily: read(style, '--su-font-sans', 'system-ui, sans-serif'),
    text: read(style, '--su-text'),
    textMuted: read(style, '--su-text-muted'),
    textSubtle: read(style, '--su-text-subtle'),
    surface1: read(style, '--su-surface-1'),
    surface2: read(style, '--su-surface-2'),
    surface3: read(style, '--su-surface-3'),
    hairline: read(style, '--su-hairline'),
    borderStrong: read(style, '--su-border-strong'),
    accent: read(style, '--su-accent'),
    palette: Array.from({ length: PALETTE_SIZE }, (_, i) => read(style, `--su-chart-${i + 1}`)),
    divergingNeg: read(style, '--su-diverging-neg'),
    divergingMid: read(style, '--su-diverging-mid'),
    divergingPos: read(style, '--su-diverging-pos'),
    neutralBar: read(style, '--su-chart-neutral-bar'),
    success: read(style, '--su-success'),
    warning: read(style, '--su-warning'),
    danger: read(style, '--su-danger'),
    info: read(style, '--su-info'),
  };
}

/** Parses `#rgb`, `#rrggbb` or `rgb(a)(...)` into 0..255 channels and alpha. Returns null for anything else. */
export function parseCssColor(value: string): { r: number; g: number; b: number; a: number } | null {
  const v = value.trim().toLowerCase();
  let m = /^#([0-9a-f]{6})$/.exec(v);
  if (m?.[1]) {
    const n = parseInt(m[1], 16);
    return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255, a: 1 };
  }
  m = /^#([0-9a-f]{3})$/.exec(v);
  if (m?.[1]) {
    const [r, g, b] = [...m[1]].map((c) => parseInt(c + c, 16));
    return { r: r ?? 0, g: g ?? 0, b: b ?? 0, a: 1 };
  }
  const f = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)(?:\s*[/,]\s*([\d.]+%?))?\s*\)$/.exec(v);
  if (!f) return null;
  const alpha = f[4] === undefined ? 1 : f[4].endsWith('%') ? Number.parseFloat(f[4]) / 100 : Number(f[4]);
  return { r: Number(f[1]), g: Number(f[2]), b: Number(f[3]), a: alpha };
}

/** `color` with an alpha, as an `rgba()` string. Unparseable colours are returned unchanged. */
export function withAlpha(color: string, alpha: number): string {
  const c = parseCssColor(color);
  return c ? `rgba(${Math.round(c.r)}, ${Math.round(c.g)}, ${Math.round(c.b)}, ${alpha})` : color;
}

/** Linear mix of two colours (t = 0 gives `from`, 1 gives `to`); alpha is ignored. */
export function mixColors(from: string, to: string, t: number): string {
  const a = parseCssColor(from);
  const b = parseCssColor(to);
  if (!a || !b) return to;
  const ch = (x: number, y: number): string =>
    Math.round(x + (y - x) * t)
      .toString(16)
      .padStart(2, '0');
  return `#${ch(a.r, b.r)}${ch(a.g, b.g)}${ch(a.b, b.b)}`;
}

/** Six-step sequential ramp for heatmaps: surface-2 to the accent (dark: lightness rises; light: it falls). */
export function sequentialRamp(tokens: Pick<SuiteChartTokens, 'surface2' | 'accent'>, steps = 6): string[] {
  return Array.from({ length: steps }, (_, i) => mixColors(tokens.surface2, tokens.accent, i / (steps - 1)));
}

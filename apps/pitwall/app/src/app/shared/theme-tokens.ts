/**
 * The colours charts need in TypeScript (ECharts takes values, not CSS variables): a copy of @decryp7/paddock-ui's tokens
 * for Pitwall's accent, translucent ones drawn over the card. `scripts/ci/tokens.test.mjs` derives every value from the
 * shared `_tokens.scss` and fails when this copy differs (`node scripts/ci/tokens.mjs` prints the values to paste). The
 * shared token tests measure contrast and colour-vision separation; `theme-tokens.spec.ts` checks what Pitwall's charts
 * rely on. The series are the family's categorical palette in its fixed order; series take slots in order and never cycle.
 */

export type ResolvedTheme = 'light' | 'dark';

export interface ThemeTokens {
  /** Card background charts are drawn on. */
  surface: string;
  /** The window / content background. */
  window: string;
  text: string;
  text2: string;
  text3: string;
  accent: string;
  ok: string;
  warn: string;
  bad: string;
  info: string;
  /** The tinted pill backgrounds, as drawn over the card (the dark theme's translucent tints, blended). */
  okTint: string;
  warnTint: string;
  badTint: string;
  infoTint: string;
  grid: string;
  axis: string;
  tooltipBg: string;
  tooltipText: string;
  /** Categorical series colours, fixed order. */
  series: readonly string[];
  /**
   * The memory history's three lines: RAM, VRAM, Pitwall (the first three slots of the family palette). Pitwall's own
   * line is also dashed, so the three never depend on hue alone.
   */
  history: readonly [string, string, string];
}

export const THEME_TOKENS: Record<ResolvedTheme, ThemeTokens> = {
  light: {
    surface: '#ffffff',
    window: '#f4f5f7',
    text: '#14171c',
    text2: '#4f5764',
    text3: '#5f6775',
    accent: '#6d3fd6',
    ok: '#187a46',
    warn: '#8f5f00',
    bad: '#c42b2b',
    info: '#1f5fa8',
    okTint: '#e3f5eb',
    warnTint: '#fbf0d6',
    badTint: '#fce6e6',
    infoTint: '#e4eef9',
    grid: '#e7e8e9',
    axis: '#cacbce',
    tooltipBg: '#14171c',
    tooltipText: '#ffffff',
    series: ['#0c6dbe', '#c47600', '#087d51', '#b04c95', '#be3108', '#494fdb', '#816a1d', '#6f737d'],
    history: ['#0c6dbe', '#c47600', '#087d51'],
  },
  dark: {
    surface: '#15171b',
    window: '#0e0f12',
    text: '#eceef1',
    text2: '#a0a7b2',
    text3: '#8b93a0',
    accent: '#a58bfa',
    ok: '#3dd68c',
    warn: '#f2b84b',
    bad: '#ff6b6b',
    info: '#7db2e8',
    okTint: '#1b322b',
    warnTint: '#342e22',
    badTint: '#362326',
    infoTint: '#242d38',
    grid: '#282a2d',
    axis: '#3a3c3f',
    tooltipBg: '#eceef1',
    tooltipText: '#15171b',
    series: ['#60aeeb', '#f3aa25', '#3dd39d', '#d68cc2', '#f07d3a', '#7a70ff', '#e8dc5a', '#a2acb1'],
    history: ['#60aeeb', '#f3aa25', '#3dd39d'],
  },
};

/** Slot of a series by a stable key order (colour follows the entity, never its rank). */
export function seriesColor(tokens: ThemeTokens, index: number): string {
  const s = tokens.series;
  // Past the last slot a series folds into the last colour; callers fold extra series into "Other" first.
  return s[Math.min(index, s.length - 1)]!;
}

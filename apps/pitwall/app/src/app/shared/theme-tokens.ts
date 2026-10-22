/**
 * The colours charts need in TypeScript, mirrored from styles.scss (keep both in step). `theme-tokens.spec.ts`
 * checks WCAG AA for text on surfaces and the colour-vision-deficiency separation of the categorical palette in both
 * themes. The series palette is a validated categorical palette (fixed order: the order is what keeps neighbours
 * distinguishable under protanopia and deuteranopia); series take slots in order and never cycle.
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
  grid: string;
  axis: string;
  tooltipBg: string;
  tooltipText: string;
  /** Categorical series colours, fixed order. */
  series: readonly string[];
}

export const THEME_TOKENS: Record<ResolvedTheme, ThemeTokens> = {
  light: {
    surface: '#ffffff',
    window: '#f4f5f7',
    text: '#15171c',
    text2: '#505561',
    text3: '#686d78',
    accent: '#5a24b0',
    ok: '#17803d',
    warn: '#9a5b00',
    bad: '#c8293a',
    grid: '#eef0f3',
    axis: '#d5d8de',
    tooltipBg: '#1b1d23',
    tooltipText: '#f3f4f6',
    series: ['#2a78d6', '#eb6834', '#1baf7a', '#eda100', '#e87ba4', '#008300', '#6250d6', '#e34948'],
  },
  dark: {
    surface: '#141519',
    window: '#0b0c0f',
    text: '#ecedf0',
    text2: '#a3a7b0',
    text3: '#858a94',
    accent: '#a98bf2',
    ok: '#4cc77a',
    warn: '#e7a83b',
    bad: '#ff6b72',
    grid: '#1f2126',
    axis: '#2e3138',
    tooltipBg: '#e9eaee',
    tooltipText: '#121317',
    series: ['#3987e5', '#d95926', '#199e70', '#c98500', '#d55181', '#008300', '#9085e9', '#e66767'],
  },
};

/** Slot of a series by a stable key order (colour follows the entity, never its rank). */
export function seriesColor(tokens: ThemeTokens, index: number): string {
  const s = tokens.series;
  // Past the last slot a series folds into the last colour; callers fold extra series into "Other" first.
  return s[Math.min(index, s.length - 1)]!;
}

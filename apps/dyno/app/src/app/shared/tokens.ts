/**
 * The colour tokens that carry text, icons and chart marks, per theme: a copy of the values in `styles.scss` for the
 * contrast and colour-vision checks in tokens.spec.ts and palette.spec.ts. Change both together.
 */
export interface ThemeTokens {
  bg: string;
  surface1: string;
  surface2: string;
  surface3: string;
  inset: string;
  overlay: string;
  text1: string;
  text2: string;
  text3: string;
  accent: string;
  accentText: string;
  accentSoft: string;
  textOnAccent: string;
  ok: string;
  okSoft: string;
  warn: string;
  warnSoft: string;
  bad: string;
  badSoft: string;
  info: string;
  infoSoft: string;
  border: string;
  borderStrong: string;
  chartGrid: string;
  focus: string;
  chart: readonly string[];
}

export const LIGHT: ThemeTokens = {
  bg: '#f4f4f6',
  surface1: '#eeeef1',
  surface2: '#ffffff',
  surface3: '#f4f4f6',
  inset: '#f6f6f8',
  overlay: '#ffffff',
  text1: '#16171a',
  text2: '#50535b',
  text3: '#686b73',
  accent: '#be3584',
  accentText: '#a0226a',
  accentSoft: '#fbeaf3',
  textOnAccent: '#ffffff',
  ok: '#157347',
  okSoft: '#e3f4ea',
  warn: '#965400',
  warnSoft: '#fbefd9',
  bad: '#c22f28',
  badSoft: '#fde6e4',
  info: '#1d63b0',
  infoSoft: '#e2eefb',
  border: '#e2e2e7',
  borderStrong: '#cdced5',
  chartGrid: '#ececf0',
  focus: '#be3584',
  chart: ['#0b6fb0', '#c25700', '#00845f', '#6b7785', '#2b2d42', '#7a5cc4'],
};

export const DARK: ThemeTokens = {
  bg: '#0e0f11',
  surface1: '#0e0f11',
  surface2: '#17181b',
  surface3: '#1d1e22',
  inset: '#131416',
  overlay: '#1b1c20',
  text1: '#ededf0',
  text2: '#a9abb3',
  text3: '#8c8f98',
  accent: '#de559f',
  accentText: '#ef86be',
  accentSoft: '#2c1422',
  textOnAccent: '#1d0614',
  ok: '#4cc58b',
  okSoft: '#12291e',
  warn: '#eaa64a',
  warnSoft: '#2e2211',
  bad: '#f7716a',
  badSoft: '#351817',
  info: '#62aef5',
  infoSoft: '#132538',
  border: '#26272c',
  borderStrong: '#35363d',
  chartGrid: '#222327',
  focus: '#ef86be',
  chart: ['#56b4e9', '#e69f00', '#5cc97a', '#9eabbc', '#e3d24c', '#a993f0'],
};

/**
 * Colour math for checking the design tokens: WCAG contrast, OKLab, and colour-vision-deficiency simulation
 * (Machado, Oliveira & Fernandes 2009, severity 1.0). Pure functions; used by tests and nothing at run time that
 * matters for behaviour.
 */

export type Cvd = 'protan' | 'deutan' | 'tritan';
type Rgb = readonly [number, number, number];
type Matrix = readonly [Rgb, Rgb, Rgb];

const MACHADO: Record<Cvd, Matrix> = {
  protan: [
    [0.152286, 1.052583, -0.204868],
    [0.114503, 0.786281, 0.099216],
    [-0.003882, -0.048116, 1.051998],
  ],
  deutan: [
    [0.367322, 0.860646, -0.227968],
    [0.280085, 0.672501, 0.047413],
    [-0.01182, 0.04294, 0.968881],
  ],
  tritan: [
    [1.255528, -0.076749, -0.178779],
    [-0.078411, 0.930809, 0.147602],
    [0.004733, 0.691367, 0.3039],
  ],
};

/** `#rrggbb` (with or without `#`) as sRGB channels 0–1; throws on anything else so a typo fails the test. */
export function parseHex(hex: string): Rgb {
  const h = hex.trim().replace(/^#/, '');
  if (!/^[0-9a-fA-F]{6}$/.test(h)) throw new Error(`not a #rrggbb colour: ${hex}`);
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255) as unknown as Rgb;
}

const toLinear = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const linear = (hex: string): Rgb => parseHex(hex).map(toLinear) as unknown as Rgb;

/** WCAG relative luminance. */
export function luminance(hex: string): number {
  const [r, g, b] = linear(hex);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG contrast ratio of two colours (1–21). */
export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

function oklab([r, g, b]: Rgb): Rgb {
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

const clamp01 = (c: number) => Math.max(0, Math.min(1, c));

/** A colour as seen with a colour-vision deficiency, in linear RGB. */
export function simulate(hex: string, kind: Cvd): Rgb {
  const [r, g, b] = linear(hex);
  const m = MACHADO[kind];
  return m.map((row) => clamp01(row[0] * r + row[1] * g + row[2] * b)) as unknown as Rgb;
}

/** Perceptual distance: Euclidean in OKLab ×100, optionally under a simulated deficiency. */
export function deltaE(a: string, b: string, kind?: Cvd): number {
  const pa = oklab(kind ? simulate(a, kind) : linear(a));
  const pb = oklab(kind ? simulate(b, kind) : linear(b));
  return 100 * Math.hypot(pa[0] - pb[0], pa[1] - pb[1], pa[2] - pb[2]);
}

/** The worst distance between neighbouring palette slots (lines, stacks and bars only put neighbours together). */
export function worstAdjacent(palette: readonly string[], kind?: Cvd): number {
  let worst = Infinity;
  for (let i = 1; i < palette.length; i++) worst = Math.min(worst, deltaE(palette[i - 1]!, palette[i]!, kind));
  return worst;
}

/**
 * Colour maths for the design-system checks (tokens.spec.ts, palette.spec.ts): WCAG contrast, colour-vision
 * deficiency simulation and perceptual distance. Pure functions over `#rrggbb`.
 */

export type Rgb = readonly [number, number, number];

/** `#rrggbb` → 0–255 channels. */
export function parseHex(hex: string): Rgb {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) throw new Error(`Not a #rrggbb colour: ${hex}`);
  const n = parseInt(m[1]!, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

const toLinear = (c: number) => {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
};
const toSrgb = (l: number) => {
  const c = l <= 0.0031308 ? l * 12.92 : 1.055 * l ** (1 / 2.4) - 0.055;
  return Math.round(Math.min(1, Math.max(0, c)) * 255);
};

/** WCAG 2 relative luminance. */
export function luminance(rgb: Rgb): number {
  const [r, g, b] = rgb.map(toLinear) as [number, number, number];
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG 2 contrast ratio of two colours (1–21). */
export function contrast(a: string, b: string): number {
  const la = luminance(parseHex(a));
  const lb = luminance(parseHex(b));
  const [hi, lo] = la > lb ? [la, lb] : [lb, la];
  return (hi + 0.05) / (lo + 0.05);
}

export type Deficiency = 'protanopia' | 'deuteranopia' | 'tritanopia';

/** Machado, Oliveira & Fernandes (2009) matrices at full severity, applied in linear RGB. */
const MATRICES: Record<Deficiency, readonly number[]> = {
  protanopia: [0.152286, 1.052583, -0.204868, 0.114503, 0.786281, 0.099216, -0.003882, -0.048116, 1.051998],
  deuteranopia: [0.367322, 0.860646, -0.227968, 0.280085, 0.672501, 0.047413, -0.01182, 0.04294, 0.968881],
  tritanopia: [1.255528, -0.076749, -0.178779, -0.078411, 0.930809, 0.147602, 0.004733, 0.691367, 0.3039],
};

/** How a colour looks with a colour-vision deficiency. */
export function simulate(hex: string, kind: Deficiency): Rgb {
  const [r, g, b] = parseHex(hex).map(toLinear) as [number, number, number];
  const m = MATRICES[kind];
  return [toSrgb(m[0]! * r + m[1]! * g + m[2]! * b), toSrgb(m[3]! * r + m[4]! * g + m[5]! * b), toSrgb(m[6]! * r + m[7]! * g + m[8]! * b)];
}

/** sRGB → CIE L*a*b* (D65). */
export function toLab(rgb: Rgb): [number, number, number] {
  const [r, g, b] = rgb.map(toLinear) as [number, number, number];
  const x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
  const y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  const z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
  const f = (t: number) => (t > 216 / 24389 ? Math.cbrt(t) : (24389 / 27) * t / 116 + 16 / 116);
  const [fx, fy, fz] = [f(x), f(y), f(z)];
  return [116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz)];
}

/** CIE76 colour difference (ΔE*ab); ~10+ reads as clearly different at chart-line sizes. */
export function deltaE(a: Rgb, b: Rgb): number {
  const [l1, a1, b1] = toLab(a);
  const [l2, a2, b2] = toLab(b);
  return Math.hypot(l1 - l2, a1 - a2, b1 - b2);
}

/** `#rrggbb` → `rgba(r, g, b, alpha)`. */
export function withAlpha(hex: string, alpha: number): string {
  const [r, g, b] = parseHex(hex);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

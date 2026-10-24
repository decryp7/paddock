/**
 * Colour maths for the token tests and the theme-pack checks (`themeProblems`): sRGB parsing, WCAG contrast, Machado CVD
 * simulation, CIE Lab distance. Pure; no DOM.
 */

export interface Rgba {
  r: number; // 0..255
  g: number;
  b: number;
  a: number; // 0..1
}

export function parseColor(value: string): Rgba {
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
  const f = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)(?:\s*[/,]\s*([\d.]+))?\s*\)$/.exec(v);
  if (f) {
    return { r: Number(f[1]), g: Number(f[2]), b: Number(f[3]), a: f[4] === undefined ? 1 : Number(f[4]) };
  }
  throw new Error(`Unsupported colour: ${value}`);
}

/** `parseColor` for untrusted values: `null` instead of an exception for anything that is not a hex or rgb() colour. */
export function tryParseColor(value: string): Rgba | null {
  try {
    const c = parseColor(value);
    return [c.r, c.g, c.b].every((x) => x >= 0 && x <= 255) && c.a >= 0 && c.a <= 1 ? c : null;
  } catch {
    return null;
  }
}

/** Source-over compositing of `fg` on an opaque `bg`. */
export function over(fg: Rgba, bg: Rgba): Rgba {
  const k = fg.a;
  return { r: fg.r * k + bg.r * (1 - k), g: fg.g * k + bg.g * (1 - k), b: fg.b * k + bg.b * (1 - k), a: 1 };
}

const lin = (c: number): number => {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
};
const delin = (l: number): number => {
  const c = Math.min(1, Math.max(0, l));
  return 255 * (c <= 0.0031308 ? c * 12.92 : 1.055 * c ** (1 / 2.4) - 0.055);
};

export function luminance(c: Rgba): number {
  return 0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b);
}

/** WCAG 2.x contrast ratio; translucent colours are composited over `bg` first. */
export function contrast(fg: Rgba, bg: Rgba): number {
  const b = over(bg, { r: 0, g: 0, b: 0, a: 1 });
  const f = over(fg, b);
  const [hi, lo] = [luminance(f), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

export type Cvd = 'protanopia' | 'deuteranopia' | 'tritanopia';

// Machado, Oliveira, Fernandes (2009), severity 1.0, applied in linear RGB.
const MATRICES: Record<Cvd, number[][]> = {
  protanopia: [
    [0.152286, 1.052583, -0.204868],
    [0.114503, 0.786281, 0.099216],
    [-0.003882, -0.048116, 1.051998],
  ],
  deuteranopia: [
    [0.367322, 0.860646, -0.227968],
    [0.280085, 0.672501, 0.047413],
    [-0.01182, 0.04294, 0.968881],
  ],
  tritanopia: [
    [1.255528, -0.076749, -0.178779],
    [-0.078411, 0.930809, 0.147602],
    [0.004733, 0.691367, 0.3039],
  ],
};

export function simulate(c: Rgba, kind: Cvd): Rgba {
  const m = MATRICES[kind];
  const v = [lin(c.r), lin(c.g), lin(c.b)];
  const out = m.map((row) => row.reduce((s, k, i) => s + k * (v[i] ?? 0), 0));
  return { r: delin(out[0] ?? 0), g: delin(out[1] ?? 0), b: delin(out[2] ?? 0), a: 1 };
}

export function lab(c: Rgba): [number, number, number] {
  const [r, g, b] = [lin(c.r), lin(c.g), lin(c.b)];
  const x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047;
  const y = 0.2126729 * r + 0.7151522 * g + 0.072175 * b;
  const z = (0.0193339 * r + 0.119192 * g + 0.9503041 * b) / 1.08883;
  const f = (t: number): number => (t > 216 / 24389 ? Math.cbrt(t) : (24389 / 27 * t + 16) / 116);
  const [fx, fy, fz] = [f(x), f(y), f(z)];
  return [116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz)];
}

/** CIE76 colour difference (Euclidean distance in Lab). */
export function deltaE(a: Rgba, b: Rgba): number {
  const [l1, a1, b1] = lab(a);
  const [l2, a2, b2] = lab(b);
  return Math.hypot(l1 - l2, a1 - a2, b1 - b2);
}

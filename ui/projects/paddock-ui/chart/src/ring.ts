/**
 * Colour order for a ring (donut): neighbouring slices must stay apart for people with colour-vision deficiency (the guide:
 * ΔE ≥ 20 for adjacent pairs under protanopia, deuteranopia and tritanopia). A ring closes on itself, so the last slice
 * borders the first. Pure and free of imports so the token tests can run it over the real palettes.
 */

type Rgb = readonly [number, number, number];
type Matrix = readonly [Rgb, Rgb, Rgb];

/** Machado, Oliveira and Fernandes (2009), severity 1.0, applied in linear RGB. */
const MACHADO: Readonly<Record<'protan' | 'deutan' | 'tritan', Matrix>> = {
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

/** The vision types the order is checked for. */
export const RING_VISIONS = ['normal', 'protan', 'deutan', 'tritan'] as const;
export type RingVision = (typeof RING_VISIONS)[number];

/** The ΔE the guide asks of adjacent slices. */
export const RING_MIN_DELTA_E = 20;

const toLinear = (c: number): number => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const fromLinear = (c: number): number => (c <= 0.0031308 ? c * 12.92 : 1.055 * c ** (1 / 2.4) - 0.055);

/** `#rrggbb` to 0..1 channels, or null for anything else. */
export function hexToRgb(hex: string): Rgb | null {
  const m = /^#([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m?.[1]) return null;
  const n = parseInt(m[1], 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

/** How a colour looks to a viewer with the given vision (0..1 sRGB channels). */
export function simulateVision(rgb: Rgb, vision: RingVision): Rgb {
  if (vision === 'normal') return rgb;
  const m = MACHADO[vision];
  const lin = rgb.map(toLinear);
  const out = m.map((row) => Math.min(1, Math.max(0, row[0] * (lin[0] ?? 0) + row[1] * (lin[1] ?? 0) + row[2] * (lin[2] ?? 0))));
  return [fromLinear(out[0] ?? 0), fromLinear(out[1] ?? 0), fromLinear(out[2] ?? 0)];
}

/** CIELAB (D65) of sRGB channels. */
function lab(rgb: Rgb): readonly [number, number, number] {
  const [r, g, b] = rgb.map(toLinear) as [number, number, number];
  const x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047;
  const y = 0.2126729 * r + 0.7151522 * g + 0.072175 * b;
  const z = (0.0193339 * r + 0.119192 * g + 0.9503041 * b) / 1.08883;
  const f = (t: number): number => (t > 216 / 24389 ? Math.cbrt(t) : (24389 / 27 * t + 16) / 116);
  const [fx, fy, fz] = [f(x), f(y), f(z)] as [number, number, number];
  return [116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz)];
}

/** CIE76 ΔE between two colours as seen with `vision`. */
export function deltaE(a: Rgb, b: Rgb, vision: RingVision = 'normal'): number {
  const [la, lb] = [lab(simulateVision(a, vision)), lab(simulateVision(b, vision))];
  return Math.hypot(la[0] - lb[0], la[1] - lb[1], la[2] - lb[2]);
}

/** The smallest ΔE between neighbours of the ring `colours` (the last borders the first), over every vision type. */
export function ringMinDeltaE(colours: readonly Rgb[]): number {
  if (colours.length < 2) return Infinity;
  const pairs = colours.length === 2 ? 1 : colours.length;
  let min = Infinity;
  for (let i = 0; i < pairs; i += 1) {
    const [a, b] = [colours[i], colours[(i + 1) % colours.length]];
    if (!a || !b) continue;
    for (const v of RING_VISIONS) min = Math.min(min, deltaE(a, b, v));
  }
  return min;
}

/** The most slices a ring colours distinctly; more are merged into one "Other" by `chartOption`. */
export const RING_MAX_SLICES = 5;

/**
 * Which palette slots colour `n` slices, in order. The palette's own order when it already keeps every neighbour (the wrap-around
 * pair included) apart; otherwise the order of the first seven slots with the largest smallest ΔE (ties: the lowest slots first).
 * Palette values that are not `#rrggbb` give the plain order.
 */
export function ringOrder(palette: readonly string[], n: number): readonly number[] {
  const plain = Array.from({ length: n }, (_, i) => i);
  const rgb = palette.slice(0, 7).map(hexToRgb);
  if (n < 2 || rgb.length < n || rgb.some((c) => c === null)) return plain;
  const colours = rgb as Rgb[];
  const score = (order: readonly number[]): number => ringMinDeltaE(order.map((i) => colours[i] as Rgb));
  if (score(plain) >= RING_MIN_DELTA_E) return plain;
  let best = plain;
  let bestScore = score(plain);
  const used = new Array<boolean>(colours.length).fill(false);
  const current: number[] = [];
  const walk = (): void => {
    if (current.length === n) {
      const s = score(current);
      if (s > bestScore + 1e-9) {
        best = [...current];
        bestScore = s;
      }
      return;
    }
    for (let i = 0; i < colours.length; i += 1) {
      if (used[i]) continue;
      used[i] = true;
      current.push(i);
      walk();
      current.pop();
      used[i] = false;
    }
  };
  walk();
  return best;
}

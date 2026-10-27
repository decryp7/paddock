/**
 * Deterministic pseudo-random helpers for the mock backend: the same inputs always give the same output, so
 * screenshots with a frozen clock are stable and tests can assert exact values.
 */

/** A hash of an integer and a seed to `[0, 1)`. */
export function hash01(n: number, seed: number): number {
  let h = Math.imul(Math.floor(n) ^ 0x9e3779b9, 0x85ebca6b) ^ Math.imul(seed + 0x632be5ab, 0xc2b2ae35);
  h = Math.imul(h ^ (h >>> 16), 0x7feb352d);
  h = Math.imul(h ^ (h >>> 15), 0x846ca68b);
  h ^= h >>> 16;
  return (h >>> 0) / 4294967296;
}

/** Smooth value noise in `[0, 1)`: hashed knots every `period` units of `t`, eased in between. */
export function smoothNoise(t: number, seed: number, period = 8): number {
  const x = t / period;
  const i = Math.floor(x);
  const f = x - i;
  const u = f * f * (3 - 2 * f);
  const a = hash01(i, seed);
  const b = hash01(i + 1, seed);
  return a + (b - a) * u;
}

/** Two octaves of {@link smoothNoise}, centred on 0 (`[-1, 1)`): slow drift plus a little jitter. */
export function wobble(t: number, seed: number, period = 8): number {
  return (smoothNoise(t, seed, period) * 0.7 + smoothNoise(t, seed + 101, period / 4) * 0.3) * 2 - 1;
}

/** A seeded PRNG (mulberry32) for one-off generated data. */
export function prng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Clamps `v` to `[lo, hi]`. */
export function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

/** A stable small integer for a string (seeds per process / app). */
export function seedOf(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619);
  return h >>> 0;
}

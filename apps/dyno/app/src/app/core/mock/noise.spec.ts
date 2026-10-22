import { clamp, hash01, prng, seedOf, smoothNoise, wobble } from './noise';

describe('mock noise', () => {
  it('hash01 is deterministic and in [0, 1)', () => {
    expect(hash01(5, 1)).toBe(hash01(5, 1));
    expect(hash01(5, 1)).not.toBe(hash01(5, 2));
    expect(hash01(5, 1)).not.toBe(hash01(6, 1));
    for (let i = -50; i < 50; i++) {
      const v = hash01(i, 3);
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThan(1);
    }
  });
  it('smoothNoise matches the hashed knots and is continuous', () => {
    expect(smoothNoise(16, 9, 8)).toBeCloseTo(hash01(2, 9), 10);
    expect(Math.abs(smoothNoise(16.001, 9, 8) - smoothNoise(16, 9, 8))).toBeLessThan(0.01);
    expect(smoothNoise(3, 9)).toBe(smoothNoise(3, 9));
  });
  it('wobble stays in [-1, 1) and is deterministic', () => {
    for (let t = 0; t < 500; t += 7) {
      const v = wobble(t, 4, 6);
      expect(v).toBeGreaterThanOrEqual(-1);
      expect(v).toBeLessThan(1);
    }
    expect(wobble(10, 1)).toBe(wobble(10, 1));
  });
  it('prng repeats per seed and differs across seeds', () => {
    const a = prng(7);
    const b = prng(7);
    const seq = [a(), a(), a()];
    expect(seq).toEqual([b(), b(), b()]);
    expect(prng(8)()).not.toBe(seq[0]);
    expect(seq.every((v) => v >= 0 && v < 1)).toBe(true);
  });
  it('clamp bounds the value', () => {
    expect(clamp(5, 0, 3)).toBe(3);
    expect(clamp(-1, 0, 3)).toBe(0);
    expect(clamp(2, 0, 3)).toBe(2);
  });
  it('seedOf is a stable small hash per string', () => {
    expect(seedOf('rustc')).toBe(seedOf('rustc'));
    expect(seedOf('rustc')).not.toBe(seedOf('cargo'));
    expect(seedOf('')).toBe(2166136261);
  });
});

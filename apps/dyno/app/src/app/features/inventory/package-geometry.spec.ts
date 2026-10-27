import { applePackage, coreSplit, grid, isAppleSilicon, parseAppleChip, solderedBoard, type Rect } from './package-geometry';

const GB = 1024 ** 3;
const inside = (c: Rect, r: Rect) => c.x >= r.x - 0.01 && c.y >= r.y - 0.01 && c.x + c.w <= r.x + r.w + 0.01 && c.y + c.h <= r.y + r.h + 0.01;

describe('Apple chips', () => {
  it('reads the generation and tier from the name', () => {
    expect(parseAppleChip('Apple M4 Pro')).toEqual({ generation: 4, tier: 'pro' });
    expect(parseAppleChip('Apple M1')).toEqual({ generation: 1, tier: 'base' });
    expect(parseAppleChip('Apple M2 Ultra')).toEqual({ generation: 2, tier: 'ultra' });
    expect(parseAppleChip('Apple M3 Max')?.tier).toBe('max');
    expect(parseAppleChip('Intel Core i9')).toBeNull();
    expect(isAppleSilicon('Apple A18')).toBe(true);
    expect(isAppleSilicon('AMD Ryzen 7')).toBe(false);
  });

  it('splits the cores into performance and efficiency', () => {
    expect(coreSplit({ generation: 4, tier: 'pro' }, 14)).toEqual({ performance: 10, efficiency: 4 });
    expect(coreSplit({ generation: 4, tier: 'base' }, 10)).toEqual({ performance: 4, efficiency: 6 });
    expect(coreSplit({ generation: 1, tier: 'base' }, 8)).toEqual({ performance: 4, efficiency: 4 });
    expect(coreSplit({ generation: 3, tier: 'pro' }, 12)).toEqual({ performance: 6, efficiency: 6 });
    expect(coreSplit({ generation: 1, tier: 'pro' }, 10)).toEqual({ performance: 8, efficiency: 2 });
    expect(coreSplit({ generation: 2, tier: 'max' }, 12)).toEqual({ performance: 8, efficiency: 4 });
    expect(coreSplit({ generation: 1, tier: 'max' }, 10)).toEqual({ performance: 8, efficiency: 2 });
    expect(coreSplit({ generation: 1, tier: 'ultra' }, 20)).toEqual({ performance: 16, efficiency: 4 });
    expect(coreSplit({ generation: 2, tier: 'ultra' }, 24)).toEqual({ performance: 16, efficiency: 8 });
  });

  it('gives no split when unknown or when the numbers do not add up', () => {
    expect(coreSplit(null, 8)).toBeNull();
    expect(coreSplit({ generation: 4, tier: 'base' }, 1)).toBeNull();
    expect(coreSplit({ generation: 4, tier: 'base' }, 4)).toBeNull();
  });
});

describe('grid', () => {
  it('fits n cells inside the rectangle', () => {
    const r = { x: 10, y: 10, w: 60, h: 30 };
    for (const n of [1, 4, 10, 16]) {
      const cells = grid(r, n);
      expect(cells).toHaveLength(n);
      expect(cells.every((c) => inside(c, r) && c.w > 0 && c.h > 0)).toBe(true);
    }
    expect(grid(r, 0)).toEqual([]);
    expect(grid({ x: 0, y: 0, w: 4, h: 4 }, 3)).toEqual([]);
  });
});

describe('applePackage', () => {
  it('draws the M4 Pro with 14 cores, a GPU, a neural engine and two memory packages', () => {
    const l = applePackage('Apple M4 Pro', 14, 48 * GB, 'lpddr5');
    expect(l.kind).toBe('apple');
    expect(l.dies).toHaveLength(1);
    expect(l.memory).toHaveLength(2);
    const cells = (kind: string) => l.dies[0]!.regions.filter((r) => r.kind === kind).flatMap((r) => r.cells).length;
    expect(cells('cpu-perf')).toBe(10);
    expect(cells('cpu-eff')).toBe(4);
    expect(cells('gpu')).toBe(20);
    expect(cells('npu')).toBe(16);
    expect(l.legend.map((e) => e.label)).toEqual(['CPU', 'GPU', 'Neural Engine', 'Unified memory']);
    expect(l.legend[0]!.detail).toContain('10 performance, 4 efficiency');
    expect(l.legend[3]!.detail).toBe('48 GB LPDDR5 in 2 packages');
  });

  it('scales the die and memory with the tier', () => {
    const base = applePackage('Apple M4', 10, 16 * GB, 'lpddr5');
    const max = applePackage('Apple M3 Max', 16, 64 * GB, 'lpddr5');
    const ultra = applePackage('Apple M2 Ultra', 24, 128 * GB, 'lpddr5');
    expect(base.dies[0]!.w).toBeLessThan(max.dies[0]!.w);
    expect(max.memory).toHaveLength(4);
    expect(ultra.dies).toHaveLength(2);
    expect(ultra.memory).toHaveLength(8);
    expect(ultra.width).toBeGreaterThan(max.width);
  });

  it('keeps everything on the substrate and every core on its die', () => {
    for (const name of ['Apple M1', 'Apple M4 Pro', 'Apple M3 Max', 'Apple M2 Ultra', 'Apple M9']) {
      const l = applePackage(name, 12, 32 * GB, 'lpddr5');
      const sub = { x: 0, y: 0, w: l.width, h: l.height };
      expect(l.memory.every((m) => inside(m, sub))).toBe(true);
      for (const d of l.dies) {
        expect(inside(d, sub)).toBe(true);
        for (const r of d.regions) expect(r.cells.every((c) => inside(c, d))).toBe(true);
      }
    }
  });

  it('draws one CPU block for a chip it does not know', () => {
    const l = applePackage('Apple Silicon', 8, 16 * GB, 'unknown');
    expect(l.dies[0]!.regions.filter((r) => r.kind === 'cpu-perf' || r.kind === 'cpu-eff')).toHaveLength(1);
    expect(l.legend[0]!.detail).toBe('8 cores');
    expect(l.legend[3]!.detail).toContain('Memory');
  });
});

describe('solderedBoard', () => {
  it('draws one package per 4 GB, between 2 and 8', () => {
    expect(solderedBoard(32 * GB, 'lpddr5').memory).toHaveLength(8);
    expect(solderedBoard(16 * GB, 'lpddr5').memory).toHaveLength(4);
    expect(solderedBoard(4 * GB, 'lpddr4').memory).toHaveLength(2);
    expect(solderedBoard(128 * GB, 'lpddr5').memory).toHaveLength(8);
    expect(solderedBoard(20 * GB, 'lpddr5').memory).toHaveLength(5);
  });

  it('keeps the packages on the board and labels it soldered', () => {
    const l = solderedBoard(20 * GB, 'lpddr5');
    const board = { x: 0, y: 0, w: l.width, h: l.height };
    expect(l.kind).toBe('soldered');
    expect(l.memory.every((m) => inside(m, board))).toBe(true);
    expect(l.legend[1]!.detail).toContain('soldered');
  });
});

import { chipCount, facePath, keyGeneration, moduleGeometry, speedText, techLabel } from './dimm-geometry';

const GB = 1024 ** 3;

describe('moduleGeometry', () => {
  it('puts the key notch at a different place for every generation', () => {
    const at = (t: 'ddr2' | 'ddr3' | 'ddr4' | 'ddr5') => moduleGeometry(t, 'dimm', 8 * GB).notch.x;
    expect(new Set([at('ddr2'), at('ddr3'), at('ddr4'), at('ddr5')]).size).toBe(4);
    const so = (t: 'ddr3' | 'ddr4' | 'ddr5') => moduleGeometry(t, 'soDimm', 8 * GB).notch.x;
    expect(new Set([so('ddr3'), so('ddr4'), so('ddr5')]).size).toBe(3);
  });

  it('keys LPDDR and unknown types like a DDR generation', () => {
    expect(keyGeneration('lpddr3')).toBe('ddr3');
    expect(keyGeneration('lpddr4')).toBe('ddr4');
    expect(keyGeneration('lpddr5')).toBe('ddr5');
    expect(keyGeneration('unknown')).toBe('ddr4');
    expect(keyGeneration('ddr2')).toBe('ddr2');
  });

  it('draws a PMIC only on DDR5', () => {
    expect(moduleGeometry('ddr5', 'dimm', 16 * GB).pmic).not.toBeNull();
    expect(moduleGeometry('ddr4', 'dimm', 16 * GB).pmic).toBeNull();
    expect(moduleGeometry('lpddr5', 'soDimm', 16 * GB).pmic).not.toBeNull();
  });

  it('counts the packages by form and size', () => {
    expect(chipCount('dimm', 4 * GB)).toBe(8);
    expect(chipCount('soDimm', 8 * GB)).toBe(4);
    expect(chipCount('soDimm', 32 * GB)).toBe(8);
    expect(moduleGeometry('ddr4', 'dimm', 8 * GB).chips).toHaveLength(8);
    expect(moduleGeometry('ddr5', 'soDimm', 16 * GB).chips).toHaveLength(4);
  });

  it('keeps every package inside the module and clear of the service strip around the notch', () => {
    for (const form of ['dimm', 'soDimm'] as const) {
      for (const tech of ['ddr3', 'ddr4', 'ddr5'] as const) {
        const g = moduleGeometry(tech, form, 64 * GB);
        for (const c of g.chips) {
          expect(c.x).toBeGreaterThanOrEqual(0);
          expect(c.x + c.w).toBeLessThanOrEqual(g.width);
          expect(c.x + c.w <= g.notch.x - 8 || c.x >= g.notch.x + 8).toBe(true);
        }
        expect(g.passives.every((x) => Math.abs(x - g.notch.x) > g.notch.w)).toBe(true);
      }
    }
  });

  it('is a SO-DIMM about half as wide as a DIMM', () => {
    expect(moduleGeometry('ddr4', 'soDimm', GB).width).toBeLessThan(moduleGeometry('ddr4', 'dimm', GB).width * 0.6);
  });

  it('outlines the face with the notch cut into the bottom edge', () => {
    const g = moduleGeometry('ddr4', 'dimm', GB);
    const d = facePath(g);
    expect(d.startsWith('M3 0')).toBe(true);
    expect(d.endsWith('Z')).toBe(true);
    expect(d).toContain(`V${(g.height - g.notch.depth).toString()}`);
  });
});

describe('labels', () => {
  it('names the technology and the speeds', () => {
    expect(techLabel('lpddr5')).toBe('LPDDR5');
    expect(techLabel('unknown')).toBe('Memory');
    expect(speedText(3200, 2666)).toBe('3200 → 2666 MT/s');
    expect(speedText(2666, 2666)).toBe('2666 MT/s');
    expect(speedText(3200, null)).toBe('3200 MT/s');
    expect(speedText(null, null)).toBeNull();
  });
});

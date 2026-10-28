import { mockInventory } from '../../core/mock/inventory';
import type { MemoryArray } from '../../core/api.types';
import { memoryTech, toDimmSlots, toSocPackage } from './hardware-adapter';
import { slotModel } from './slot-model';

describe('hardware adapter', () => {
  it('maps the slots to plain models with tags and notes from the hints', () => {
    const inv = mockInventory('windows', 'ddr4');
    const slots = toDimmSlots(slotModel((inv.memory as { slots: MemoryArray }).slots, inv.hints));
    expect(slots.map((s) => s.name)).toEqual(['DIMM_A1', 'DIMM_A2', 'DIMM_B1', 'DIMM_B2']);
    expect(slots[0]!.module).toMatchObject({ tech: 'ddr4', form: 'dimm', sizeLabel: '16 GB', ratedMts: 3200, runningMts: 2666, maker: 'Corsair' });
    expect(slots[0]!.tags).toEqual([{ label: 'Speed mismatch', tone: 'warn' }]);
    expect(slots[2]!.tags).toEqual([{ label: 'Suggested slot', tone: 'info' }]);
    expect(slots[3]!.module).toBeNull();
    expect(slots[3]!.note).toBeNull();
  });

  it('marks dual-channel pairs and SO-DIMM modules', () => {
    const inv = mockInventory('linux', 'sodimm-dual');
    const slots = toDimmSlots(slotModel((inv.memory as { slots: MemoryArray }).slots, inv.hints));
    expect(slots.every((s) => s.tags.some((t) => t.label === 'Dual channel'))).toBe(true);
    expect(slots[0]!.module!.form).toBe('soDimm');
  });

  it('maps unknown kinds and describes the package', () => {
    expect(memoryTech('other')).toBe('unknown');
    expect(memoryTech('ddr5')).toBe('ddr5');
    const apple = toSocPackage({ name: 'Apple M4 Pro', physicalCores: 14, logicalCores: 14, frequencyMhz: 4500 }, { total: 48 * 1024 ** 3, kind: 'lpddr5' });
    expect(apple.style).toBe('apple');
    expect(apple.alt).toBe('Unified memory, 48 GB LPDDR5, part of the Apple M4 Pro chip package');
    const pc = toSocPackage({ name: 'Intel Core Ultra 7', physicalCores: 16, logicalCores: 22, frequencyMhz: 3800 }, { total: 32 * 1024 ** 3, kind: 'lpddr5' });
    expect(pc.style).toBe('soldered');
    expect(pc.title).toBe('Memory soldered to the mainboard');
  });
});

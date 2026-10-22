import { mockInventory } from '../../core/mock/inventory';
import type { MemoryArray, MemoryModule } from '../../core/api.types';
import { describeSlots, kindLabel, slotModel, speedLabel } from './slot-model';

const array = (inv: ReturnType<typeof mockInventory>): MemoryArray => ('slots' in inv.memory ? inv.memory.slots : (undefined as never));

describe('slotModel', () => {
  it('fills the board: two modules, two empty slots named by the hints, colours by size', () => {
    const inv = mockInventory('windows');
    const m = slotModel(array(inv), inv.hints);
    expect(m.slots.map((s) => s.name)).toEqual(['DIMM_A1', 'DIMM_A2', 'DIMM_B1', 'Slot 1']);
    expect(m.filled).toBe(2);
    expect(m.free).toBe(2);
    expect(m.slots[0]!.color).not.toBe(m.slots[1]!.color);
    expect(m.slots[2]!.hint?.kind).toBe('freeSlots');
    expect(m.slots[0]!.hint?.kind).toBe('mismatchedSpeed');
    expect(m.pairs).toEqual([]);
    expect(m.totalBytes).toBe(24 * 1024 ** 3);
  });

  it('pairs matching neighbours when the engine reports multi-channel', () => {
    const inv = mockInventory('linux');
    const m = slotModel(array(inv), inv.hints);
    expect(m.pairs).toEqual([[0, 1]]);
    expect(m.slots[0]!.color).toBe(m.slots[1]!.color);
    expect(m.free).toBe(0);
  });

  it('draws only the modules when the slot count is unknown', () => {
    const inv = mockInventory('linux');
    const a = { ...array(inv), slots: 0 };
    expect(slotModel(a, []).slots).toHaveLength(2);
    expect(slotModel({ slots: 0, maxCapacityBytes: null, modules: [] }, []).slots).toEqual([]);
  });

  it('describes the slots in words and labels types and speeds', () => {
    const inv = mockInventory('windows');
    const text = describeSlots(slotModel(array(inv), inv.hints));
    expect(text).toContain('2 of 4 memory slots filled');
    expect(text).toContain('DIMM_A1: 16 GB DDR4-2666');
    expect(text).toContain('DIMM_B1: empty');
    expect(describeSlots({ slots: [], pairs: [], filled: 0, free: 0, totalBytes: 0 })).toBe('No memory slots were reported.');
    const m = { kind: 'ddr5', speedMts: 4800, configuredSpeedMts: null } as MemoryModule;
    expect(speedLabel(m)).toBe('DDR5-4800');
    expect(speedLabel({ ...m, speedMts: null })).toBe('DDR5');
    expect(kindLabel('lpddr5')).toBe('LPDDR5');
  });
});

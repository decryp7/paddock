import { mockInventory } from '../../core/mock/inventory';
import type { MemoryArray, MemoryModule } from '../../core/api.types';
import { channelOf, describeSlots, hintLabel, inferSlotNames, kindLabel, slotModel, speedLabel, speedText } from './slot-model';

const array = (inv: ReturnType<typeof mockInventory>): MemoryArray => ('slots' in inv.memory ? inv.memory.slots : (undefined as never));

describe('slotModel', () => {
  it('fills the board: two modules, two empty slots named from the occupied names, grouped by channel', () => {
    const inv = mockInventory('windows', 'ddr4');
    const m = slotModel(array(inv), inv.hints);
    expect(m.slots.map((s) => s.name)).toEqual(['DIMM_A1', 'DIMM_A2', 'DIMM_B1', 'DIMM_B2']);
    expect(m.groups.map((g) => [g.channel, g.slots.length])).toEqual([['A', 2], ['B', 2]]);
    expect(m.filled).toBe(2);
    expect(m.free).toBe(2);
    expect(m.slots[2]!.hint?.kind).toBe('freeSlots');
    expect(m.slots[0]!.hint?.kind).toBe('mismatchedSpeed');
    expect(m.pairs).toEqual([]);
    expect(m.totalBytes).toBe(24 * 1000 ** 3);
  });

  it('pairs matching sticks across channels when the engine reports multi-channel', () => {
    const inv = mockInventory('windows', 'ddr5');
    const m = slotModel(array(inv), inv.hints);
    expect(m.slots.map((s) => s.name)).toEqual(['DIMM_A1', 'DIMM_A2', 'DIMM_B1', 'DIMM_B2']);
    expect(m.pairs).toEqual([[1, 3]]);
    expect(m.slots[1]!.paired && m.slots[3]!.paired).toBe(true);
  });

  it('pairs neighbours when the slot names carry no channel', () => {
    const mod = (slot: string): MemoryModule => ({ ...array(mockInventory('windows', 'ddr4')).modules[0]!, slot });
    const multi = [{ kind: 'multiChannel' as const, message: '', slot: null }];
    const m = slotModel({ slots: 2, maxCapacityBytes: null, modules: [mod('Slot 1'), mod('Slot 2')] }, multi);
    expect(m.pairs).toEqual([[0, 1]]);
    expect(m.groups).toHaveLength(1);
    expect(m.groups[0]!.channel).toBeNull();
  });

  it('pairs the two channel-only sticks of a dual-channel laptop', () => {
    const inv = mockInventory('linux', 'sodimm-dual');
    const m = slotModel(array(inv), inv.hints);
    expect(m.pairs).toEqual([[0, 1]]);
    expect(m.free).toBe(0);
  });

  it('draws only the modules when the slot count is unknown, and nothing for no modules', () => {
    const inv = mockInventory('linux', 'sodimm-dual');
    const a = { ...array(inv), slots: 0 };
    expect(slotModel(a, []).slots).toHaveLength(2);
    expect(slotModel({ slots: 0, maxCapacityBytes: null, modules: [] }, []).slots).toEqual([]);
  });

  it('describes the slots in words and labels types and speeds', () => {
    const inv = mockInventory('windows', 'ddr4');
    const text = describeSlots(slotModel(array(inv), inv.hints));
    expect(text).toContain('2 of 4 memory slots filled');
    expect(text).toContain('DIMM_A1: 16 GB DDR4-2666');
    expect(text).toContain('DIMM_B1: empty');
    expect(describeSlots({ slots: [], groups: [], pairs: [], filled: 0, free: 0, totalBytes: 0 })).toBe('No memory slots were reported.');
    const m = { kind: 'ddr5', speedMts: 4800, configuredSpeedMts: null } as MemoryModule;
    expect(speedLabel(m)).toBe('DDR5-4800');
    expect(speedLabel({ ...m, speedMts: null })).toBe('DDR5');
    expect(kindLabel('lpddr5')).toBe('LPDDR5');
  });

  it('writes the speed line: rated and running when they differ', () => {
    const m = { kind: 'ddr4', speedMts: 3200, configuredSpeedMts: 2666 } as MemoryModule;
    expect(speedText(m)).toBe('3200 → 2666 MT/s');
    expect(speedText({ ...m, speedMts: 2666 })).toBe('2666 MT/s');
    expect(speedText({ ...m, configuredSpeedMts: null })).toBe('3200 MT/s');
    expect(speedText({ ...m, speedMts: null, configuredSpeedMts: null })).toBeNull();
  });

  it('tags only hints that are about one slot', () => {
    expect(hintLabel({ kind: 'mismatchedSize', message: '', slot: 'A1' })).toBe('Size mismatch');
    expect(hintLabel({ kind: 'maxCapacity', message: '', slot: null })).toBeNull();
    expect(hintLabel(null)).toBeNull();
  });
});

describe('channelOf and inferSlotNames', () => {
  it('reads the channel from common slot names', () => {
    expect(channelOf('DIMM_A1')).toBe('A');
    expect(channelOf('SODIMM B')).toBe('B');
    expect(channelOf('ChannelB-DIMM0')).toBe('B');
    expect(channelOf('Slot 1')).toBeNull();
    expect(channelOf('BANK 0')).toBeNull();
  });

  it('extrapolates the missing names from the occupied ones', () => {
    expect(inferSlotNames(['DIMM_A1', 'DIMM_A2'], 4)).toEqual(['DIMM_B1', 'DIMM_B2']);
    expect(inferSlotNames(['DIMM_A2', 'DIMM_B2'], 4)).toEqual(['DIMM_A1', 'DIMM_B1']);
    expect(inferSlotNames(['DIMM_A1', 'DIMM_B1'], 2)).toEqual([]);
    expect(inferSlotNames(['dimm_a0'], 2)).toEqual(['dimm_b0']);
    expect(inferSlotNames(['SODIMM A'], 2)).toEqual(['SODIMM B']);
    expect(inferSlotNames(['Slot 1'], 3)).toEqual([]);
    expect(inferSlotNames([], 2)).toEqual([]);
    expect(inferSlotNames(['A1', 'B1'], 1)).toEqual([]);
  });

  it('falls back to Slot n when nothing can be inferred', () => {
    const mod = { ...array(mockInventory('windows', 'ddr4')).modules[0]!, slot: 'Slot 1' };
    expect(slotModel({ slots: 2, maxCapacityBytes: null, modules: [mod] }, []).slots.map((s) => s.name)).toEqual(['Slot 1', 'Slot 2']);
  });
});

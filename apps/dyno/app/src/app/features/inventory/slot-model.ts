import type { Hint, MemoryArray, MemoryKind, MemoryModule } from '../../core/api.types';
import { formatBytes } from '../../shared/format';

/** One memory slot as drawn: its name, what is in it (or nothing) and the hint that points at it. */
export interface SlotView {
  index: number;
  name: string;
  module: MemoryModule | null;
  /** Colour index (0-5) by capacity: sticks of the same size share a colour. */
  color: number;
  hint: Hint | null;
}

export interface SlotModel {
  slots: SlotView[];
  /** Pairs of neighbouring filled slots that form a dual-channel pair (equal size and type, and the engine says multi-channel). */
  pairs: [number, number][];
  filled: number;
  free: number;
  totalBytes: number;
}

const KIND_LABEL: Record<MemoryKind, string> = {
  ddr2: 'DDR2',
  ddr3: 'DDR3',
  ddr4: 'DDR4',
  ddr5: 'DDR5',
  lpddr3: 'LPDDR3',
  lpddr4: 'LPDDR4',
  lpddr5: 'LPDDR5',
  other: 'Memory',
  unknown: 'Memory',
};

export const kindLabel = (k: MemoryKind): string => KIND_LABEL[k];

/** "DDR4-3200" (the speed it runs at; the rated speed when the running speed is unknown), or just the type. */
export function speedLabel(m: MemoryModule): string {
  const mts = m.configuredSpeedMts ?? m.speedMts;
  return mts ? `${KIND_LABEL[m.kind]}-${mts.toString()}` : KIND_LABEL[m.kind];
}

/**
 * Builds the slots to draw: the modules in their slots, then empty slots up to the board's slot count (when known).
 * Colours follow the distinct capacities, smallest first, so a mismatch shows as two colours. Pure.
 */
export function slotModel(array: MemoryArray, hints: readonly Hint[]): SlotModel {
  const total = Math.max(array.slots, array.modules.length);
  const sizes = [...new Set(array.modules.map((m) => m.capacityBytes))].sort((a, b) => a - b);
  const names = new Set(array.modules.map((m) => m.slot));
  const slots: SlotView[] = [];
  array.modules.forEach((m, i) => slots.push({ index: i, name: m.slot, module: m, color: sizes.indexOf(m.capacityBytes) % 6, hint: hints.find((h) => h.slot === m.slot) ?? null }));
  let n = 1;
  while (slots.length < total) {
    let name = `Slot ${n.toString()}`;
    // Hints name free slots too ("DIMM_B1"): the empty slots take the hinted names first.
    const hinted = hints.find((h) => h.slot !== null && !names.has(h.slot) && !slots.some((s) => s.name === h.slot));
    if (hinted?.slot) name = hinted.slot;
    else n++;
    slots.push({ index: slots.length, name, module: null, color: 0, hint: hints.find((h) => h.slot === name) ?? null });
  }
  const multi = hints.some((h) => h.kind === 'multiChannel');
  const pairs: [number, number][] = [];
  for (let i = 0; multi && i + 1 < slots.length; i++) {
    const a = slots[i]!.module;
    const b = slots[i + 1]!.module;
    if (a && b && a.capacityBytes === b.capacityBytes && a.kind === b.kind) {
      pairs.push([i, i + 1]);
      i++;
    }
  }
  const filled = slots.filter((s) => s.module).length;
  return { slots, pairs, filled, free: slots.length - filled, totalBytes: array.modules.reduce((a, m) => a + m.capacityBytes, 0) };
}

/** A sentence for screen readers (and the text alternative of the drawing). */
export function describeSlots(model: SlotModel): string {
  if (model.slots.length === 0) return 'No memory slots were reported.';
  const parts = model.slots.map((s) => (s.module ? `${s.name}: ${formatBytes(s.module.capacityBytes)} ${speedLabel(s.module)}` : `${s.name}: empty`));
  return `${model.filled.toString()} of ${model.slots.length.toString()} memory slots filled, ${formatBytes(model.totalBytes)} in total. ${parts.join('. ')}.`;
}

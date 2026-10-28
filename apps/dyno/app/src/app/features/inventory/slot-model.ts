import type { Hint, MemoryArray, MemoryKind, MemoryModule } from '../../core/api.types';
import { formatMemory } from '../../shared/format';

/** One memory slot as drawn: its name, the channel it belongs to, what is in it (or nothing) and the hint that points at it. */
export interface SlotView {
  index: number;
  name: string;
  /** "A", "B"… when the slot name says so ("DIMM_A1", "SODIMM A", "ChannelB-DIMM0"); null when it does not. */
  channel: string | null;
  module: MemoryModule | null;
  hint: Hint | null;
  /** The slot is half of a dual-channel pair of matching sticks. */
  paired: boolean;
}

/** Slots that share a channel, in drawing order. The channel is null when the slot names do not carry one. */
export interface SlotGroup {
  channel: string | null;
  slots: SlotView[];
}

export interface SlotModel {
  slots: SlotView[];
  groups: SlotGroup[];
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

/** The speed line of a label: "3200 → 2666 MT/s" when the stick runs slower than rated, "2666 MT/s" otherwise, null when unknown. */
export function speedText(m: MemoryModule): string | null {
  const rated = m.speedMts;
  const running = m.configuredSpeedMts;
  if (rated && running && rated !== running) return `${rated.toString()} → ${running.toString()} MT/s`;
  const one = running ?? rated;
  return one ? `${one.toString()} MT/s` : null;
}

/** The channel letter a slot name carries ("DIMM_A1" and "SODIMM A" are A, "ChannelB-DIMM0" is B), or null. */
export function channelOf(name: string): string | null {
  const explicit = /channel[\s_-]*([A-H])\b/i.exec(name);
  if (explicit) return explicit[1]!.toUpperCase();
  const m = /(?:^|[^A-Za-z])([A-H])\d*$/i.exec(name);
  return m ? m[1]!.toUpperCase() : null;
}

const LETTERS = 'ABCDEFGH';

/**
 * Names for the slots the OS did not list (Windows only reports the occupied ones), extrapolated from the names it did: with
 * "DIMM_A1" and "DIMM_A2" on a four-slot board the missing ones are DIMM_B1 and DIMM_B2. Empty when no pattern can be read.
 */
export function inferSlotNames(existing: readonly string[], total: number): string[] {
  const missing = total - existing.length;
  if (missing <= 0 || existing.length === 0) return [];
  const taken = new Set(existing.map((n) => n.toLowerCase()));
  const out: string[] = [];
  const push = (n: string) => {
    if (out.length < missing && !taken.has(n.toLowerCase())) out.push(n);
  };
  const numbered = existing.map((n) => /^(.*?)([A-H])(\d+)$/i.exec(n));
  const first = numbered[0];
  if (first && numbered.every((m) => m !== null && m[1]!.toLowerCase() === first[1]!.toLowerCase())) {
    const prefix = first[1]!;
    const upper = first[2]! === first[2]!.toUpperCase();
    const idx = numbered.map((m) => Number(m![3]));
    const base = Math.min(...idx) === 0 ? 0 : 1;
    const perChannel = Math.max(Math.max(...idx) - base + 1, total >= 4 ? 2 : 1);
    const channels = Math.min(LETTERS.length, Math.ceil(total / perChannel));
    for (let c = 0; c < channels; c++) {
      for (let i = 0; i < perChannel; i++) {
        const letter = upper ? LETTERS[c]! : LETTERS[c]!.toLowerCase();
        push(`${prefix}${letter}${(base + i).toString()}`);
      }
    }
    return out;
  }
  const channelOnly = existing.map((n) => /^(.*?[\s_-])([A-H])$/i.exec(n));
  const head = channelOnly[0];
  if (head && channelOnly.every((m) => m !== null && m[1]!.toLowerCase() === head[1]!.toLowerCase())) {
    for (let c = 0; c < Math.min(LETTERS.length, total); c++) push(`${head[1]!}${LETTERS[c]!}`);
  }
  return out;
}

/** The slots split into runs of the same channel, in order. */
function groupBy(slots: SlotView[]): SlotView[][] {
  const out: SlotView[][] = [];
  for (const s of slots) {
    const last = out[out.length - 1];
    if (last && last[0]!.channel === s.channel) last.push(s);
    else out.push([s]);
  }
  return out;
}

const byName = (a: string, b: string) => a.localeCompare(b, 'en', { numeric: true, sensitivity: 'base' });

/**
 * Builds the slots to draw: the modules in their slots, then empty slots up to the board's slot count (when known), named by the
 * engine's hints first, then by the pattern of the occupied names, then "Slot n". When every slot names its channel they are
 * ordered by name and grouped by channel. Pure.
 */
export function slotModel(array: MemoryArray, hints: readonly Hint[]): SlotModel {
  const total = Math.max(array.slots, array.modules.length);
  const names = new Set(array.modules.map((m) => m.slot));
  const guesses = inferSlotNames(array.modules.map((m) => m.slot), total);
  const draft: { name: string; module: MemoryModule | null }[] = array.modules.map((m) => ({ name: m.slot, module: m }));
  let n = 1;
  while (draft.length < total) {
    const used = (s: string) => names.has(s) || draft.some((d) => d.name === s);
    // Hints name free slots too ("DIMM_B1"): the empty slots take the hinted names first.
    const hinted = hints.find((h) => h.slot !== null && !used(h.slot));
    const guess = guesses.find((g) => !used(g));
    let name = hinted?.slot ?? guess;
    while (!name || used(name)) name = `Slot ${(n++).toString()}`;
    draft.push({ name, module: null });
  }
  const channels = draft.map((d) => channelOf(d.name));
  if (draft.length > 0 && channels.every((c) => c !== null)) draft.sort((a, b) => byName(a.name, b.name));
  const slots: SlotView[] = draft.map((d, i) => ({
    index: i,
    name: d.name,
    channel: channelOf(d.name),
    module: d.module,
    hint: hints.find((h) => h.slot === d.name) ?? null,
    paired: false,
  }));
  const multi = hints.some((h) => h.kind === 'multiChannel');
  const pairs: [number, number][] = [];
  const same = (a: MemoryModule | null, b: MemoryModule | null) => !!a && !!b && a.capacityBytes === b.capacityBytes && a.kind === b.kind;
  const link = (i: number, j: number) => {
    pairs.push([i, j]);
    slots[i]!.paired = true;
    slots[j]!.paired = true;
  };
  if (multi) {
    // Dual channel spans two channels: slot p of channel A pairs with slot p of channel B. Without channels, neighbours pair.
    const lanes = slots.every((s) => s.channel !== null) ? groupBy(slots) : [];
    for (let c = 0; c + 1 < lanes.length; c += 2) {
      for (let p = 0; p < Math.min(lanes[c]!.length, lanes[c + 1]!.length); p++) {
        const a = lanes[c]![p]!;
        const b = lanes[c + 1]![p]!;
        if (same(a.module, b.module)) link(a.index, b.index);
      }
    }
    for (let i = 0; pairs.length === 0 && i + 1 < slots.length; i++) {
      if (same(slots[i]!.module, slots[i + 1]!.module)) {
        link(i, i + 1);
        i++;
      }
    }
  }
  const groups: SlotGroup[] = groupBy(slots).map((run) => ({ channel: run[0]!.channel, slots: run }));
  const filled = slots.filter((s) => s.module).length;
  return { slots, groups, pairs, filled, free: slots.length - filled, totalBytes: array.modules.reduce((a, m) => a + m.capacityBytes, 0) };
}

/** A sentence for screen readers (and the text alternative of the drawing). */
export function describeSlots(model: SlotModel): string {
  if (model.slots.length === 0) return 'No memory slots were reported.';
  const parts = model.slots.map((s) => (s.module ? `${s.name}: ${formatMemory(s.module.capacityBytes)} ${speedLabel(s.module)}` : `${s.name}: empty`));
  return `${model.filled.toString()} of ${model.slots.length.toString()} memory slots filled, ${formatMemory(model.totalBytes)} in total. ${parts.join('. ')}.`;
}

const HINT_LABEL: Partial<Record<Hint['kind'], string>> = {
  mismatchedSize: 'Size mismatch',
  mismatchedSpeed: 'Speed mismatch',
  belowRated: 'Below rated speed',
  freeSlots: 'Suggested slot',
};

/** The short tag a hint puts on its slot, or null when the hint is general advice rather than about one slot. */
export const hintLabel = (h: Hint | null): string | null => (h ? (HINT_LABEL[h.kind] ?? null) : null);

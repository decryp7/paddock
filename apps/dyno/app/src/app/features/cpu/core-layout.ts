import type { CoreKind } from '../../core/api.types';

/** One logical core as the UI lists it: its index in the engine's arrays, a short label and its group. */
export interface CoreSlot {
  index: number;
  label: string;
  kind: CoreKind;
}

export interface CoreGroup {
  /** `null` title: a single plain "Cores" group (uniform CPU or no topology). */
  kind: CoreKind;
  title: string;
  cores: CoreSlot[];
}

const TITLE: Record<CoreKind, string> = { performance: 'Performance cores', efficiency: 'Efficiency cores', unknown: 'Cores' };
const PREFIX: Record<CoreKind, string> = { performance: 'P', efficiency: 'E', unknown: '' };

/**
 * Groups the logical cores for display: Performance then Efficiency when the engine reports kinds, one plain group
 * otherwise (also when the kinds are all the same or unknown, or the array does not match the core count). Labels count
 * within a group (P1…, E1…) or run 1…n in a plain group. Pure.
 */
export function coreGroups(kinds: readonly CoreKind[], count: number): CoreGroup[] {
  const hybrid = kinds.length === count && new Set(kinds).size > 1;
  const kindOf = (i: number): CoreKind => (hybrid ? kinds[i]! : 'unknown');
  const order: CoreKind[] = hybrid ? ['performance', 'efficiency', 'unknown'] : ['unknown'];
  const groups: CoreGroup[] = [];
  for (const kind of order) {
    const cores: CoreSlot[] = [];
    for (let i = 0; i < count; i++) {
      if (kindOf(i) === kind) cores.push({ index: i, label: `${PREFIX[kind]}${cores.length + 1}`, kind });
    }
    if (cores.length) groups.push({ kind, title: TITLE[kind], cores });
  }
  return groups;
}

/** The cores in display order (all groups flattened). */
export function coreOrder(kinds: readonly CoreKind[], count: number): CoreSlot[] {
  return coreGroups(kinds, count).flatMap((g) => g.cores);
}

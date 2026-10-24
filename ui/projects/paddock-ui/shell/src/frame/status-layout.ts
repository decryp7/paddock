import { fitAxis } from '@decryp7/paddock-ui';

/** A status item as the bar lays it out: its measured width (px), priority, and whether it may never be hidden. */
export interface StatusSlot {
  readonly id: string;
  readonly priority: number;
  readonly width: number;
  readonly pinned?: boolean;
}

/** What fits: the ids shown (in the order given) and the ids hidden, lowest priority first. */
export interface StatusPlan {
  readonly shown: readonly string[];
  readonly hidden: readonly string[];
}

/** The room `items` take side by side, each with `gap` after it. */
export function statusWidth(items: readonly Pick<StatusSlot, 'width'>[], gap: number): number {
  return items.reduce((sum, i) => sum + Math.max(0, i.width) + gap, 0);
}

const ANCHOR = '\u0000anchor';

/**
 * Which status items fit in `available` px (plan §5.7): each takes its width plus `gap`. Pinned items always stay; of the
 * others, when they do not all fit, the lowest priority goes first (ties: the later id), and the overflow button's `more`
 * px is kept free for the rest. Pure: the bar measures (in the items' compact form when the full one does not fit), this
 * decides.
 */
export function planStatusBar(items: readonly StatusSlot[], available: number, gap: number, more: number): StatusPlan {
  const pinned = items.filter((i) => i.pinned);
  const free = Math.max(0, available - statusWidth(pinned, gap));
  // A phantom anchor of no width takes the leftover room, so every unpinned item may give way.
  const candidates = [
    { key: ANCHOR, min: 0, pref: 0, priority: Number.POSITIVE_INFINITY },
    ...items.filter((i) => !i.pinned).map((i) => ({ key: i.id, min: Math.max(0, i.width) + gap, pref: Math.max(0, i.width) + gap, priority: i.priority })),
  ];
  let result = fitAxis(candidates, free);
  if (result.dropped.length) result = fitAxis(candidates, Math.max(0, free - more));
  const dropped = new Set(result.dropped);
  return { shown: items.filter((i) => !dropped.has(i.id)).map((i) => i.id), hidden: result.dropped };
}

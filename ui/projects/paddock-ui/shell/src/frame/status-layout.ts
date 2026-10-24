import { fitAxis } from '@decryp7/paddock-ui';

/** A status item as the bar lays it out: its measured width (px) and priority. */
export interface StatusSlot {
  readonly id: string;
  readonly priority: number;
  readonly width: number;
}

/** What fits: the ids shown (in the order given) and the ids hidden, lowest priority first. */
export interface StatusPlan {
  readonly shown: readonly string[];
  readonly hidden: readonly string[];
}

/**
 * Which status items fit in `available` px (plan §5.7): each takes its width plus `gap`; when they do not all fit, the
 * lowest priority goes first (ties: the later id), and the overflow button's `more` px is kept free for the rest. The
 * highest-priority item always stays, even when it alone is too wide (it truncates). Pure: the bar measures, this decides.
 */
export function planStatusBar(items: readonly StatusSlot[], available: number, gap: number, more: number): StatusPlan {
  const fit = (room: number) =>
    fitAxis(
      items.map((i) => ({ key: i.id, min: Math.max(0, i.width) + gap, pref: Math.max(0, i.width) + gap, priority: i.priority })),
      Math.max(0, room),
    );
  let result = fit(available);
  if (result.dropped.length) result = fit(available - more);
  const dropped = new Set(result.dropped);
  return { shown: items.filter((i) => !dropped.has(i.id)).map((i) => i.id), hidden: result.dropped };
}

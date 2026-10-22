import { plural } from '../../shared/format';
import type { AdviceKind } from '../../core/api.types';

/** The change between two per-mille success rates; null when either half has no data or the rate did not change. */
export function trendText(
  earlier: number | null,
  later: number | null,
): { text: string; points: string; direction: 'up' | 'down' } | null {
  if (earlier === null || later === null) return null;
  const delta = Math.round((later - earlier) / 10);
  if (delta === 0) return null;
  const n = Math.abs(delta);
  return { text: `${delta > 0 ? 'up' : 'down'} ${plural(n, 'point')}`, points: plural(n, 'point'), direction: delta > 0 ? 'up' : 'down' };
}

/** How an advice card looks: its icon, and whether it is a real warning. */
export function adviceLook(kind: AdviceKind): { icon: 'alert' | 'check-circle'; warning: boolean } {
  switch (kind) {
    case 'spareCapacity':
      return { icon: 'check-circle', warning: false };
    case 'checkLabels':
      return { icon: 'alert', warning: false };
    case 'addCapacity':
    case 'successDropped':
      return { icon: 'alert', warning: true };
  }
}

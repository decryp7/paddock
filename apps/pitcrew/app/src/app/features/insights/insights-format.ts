import { plural } from '../../shared/format';
import type { AdviceKind } from '../../core/api.types';

/** The change between two per-mille success rates in words ("up 4 points"); null when either half has no data. */
export function trendText(earlier: number | null, later: number | null): { text: string; direction: 'up' | 'down' | 'flat' } | null {
  if (earlier === null || later === null) return null;
  const points = Math.round((later - earlier) / 10);
  if (points === 0) return { text: 'no change', direction: 'flat' };
  const n = Math.abs(points);
  return { text: `${points > 0 ? 'up' : 'down'} ${plural(n, 'point')}`, direction: points > 0 ? 'up' : 'down' };
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

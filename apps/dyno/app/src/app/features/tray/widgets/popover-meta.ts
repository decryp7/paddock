import type { PlacementId, PopoverValue, WidgetState } from '../../../core/generated/widgets';
import type { WidgetActionId } from '../../../core/generated/widgets';

/**
 * The right-hand head text of a fresh popup widget ("2 active", "3 volumes", "last 7 days"): a count or range already in the
 * engine's value, said in words. Stale and loading widgets get none from here (the frame says how old they are).
 */
export function popoverMeta(_id: PlacementId, state: WidgetState<PopoverValue, WidgetActionId>): string | null {
  if (state.state !== 'ready') return null;
  const v = state.value;
  if (typeof v !== 'object') return null;
  if ('activeAlarms' in v) {
    const n = v.activeAlarms.rows.length + v.activeAlarms.more;
    return n > 0 ? `${n} active` : null;
  }
  if ('diskSpace' in v) {
    const n = v.diskSpace.rows.length + v.diskSpace.more;
    return `${n} ${n === 1 ? 'volume' : 'volumes'}`;
  }
  if ('pinnedCounters' in v) return `${v.pinnedCounters.rows.length + v.pinnedCounters.more} pinned`;
  if ('lastIssue' in v) return 'last 7 days';
  return null;
}

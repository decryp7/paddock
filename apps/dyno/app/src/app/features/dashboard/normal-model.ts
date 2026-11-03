import type { BaselineItem, BaselineOverview, BaselineStrength } from '../../core/api.types';
import { formatClock, formatDate } from '../../shared/format';

/** Most unusual items the dashboard card lists. */
export const MAX_UNUSUAL = 4;

/** The strength of a judgement that is higher or lower than usual, or null for the other states. */
export function unusualStrength(item: BaselineItem): BaselineStrength | null {
  const s = item.judgement.state;
  if (typeof s !== 'object') return null;
  return 'higher' in s ? s.higher : s.lower;
}

/** The unusual items, strongest first (the engine already puts them first; the order within a strength is kept). */
export function unusualItems(items: readonly BaselineItem[]): BaselineItem[] {
  const rank = (i: BaselineItem) => (unusualStrength(i) === 'strong' ? 0 : 1);
  return items
    .filter((i) => unusualStrength(i) !== null)
    .map((item, idx) => ({ item, idx }))
    .sort((a, b) => rank(a.item) - rank(b.item) || a.idx - b.idx)
    .map((x) => x.item)
    .slice(0, MAX_UNUSUAL);
}

/** "CPU" for the system, "rustc CPU" for one program. */
export const itemName = (i: BaselineItem): string => (i.subject === 'System' ? i.label : `${i.subject} ${i.label}`);

/** The one quiet line while nothing is listed, or null when a list follows (or the feature is off). */
export function normalLine(o: BaselineOverview, unusualCount: number): string | null {
  const l = o.learning;
  if (l === 'off') return null;
  if ('learning' in l) {
    // The engine counts local calendar days from the first sighting (day 1 is the day Dyno first ran) and holds the last
    // day until a full week of hours has passed too: then the line says when.
    const { day, daysNeeded, since, readyAt } = l.learning;
    const head = `Learning what is normal for this computer: day ${day} of ${daysNeeded}`;
    if (day >= daysNeeded && readyAt !== null) return `${head} · ready at ${formatClock(readyAt)}`;
    return since === null ? head : `${head}, since ${formatDate(since, o.at)}`;
  }
  return unusualCount === 0 ? 'Everything looks usual for this time of the week' : null;
}

import type { ReportDisk, WeeklyReport } from '../../core/api.types';
import { formatDate } from '../../shared/format';
import { forecastText } from '../../shared/labels';

const WEEK_MS = 7 * 24 * 60 * 60 * 1000;

/** Weeks the selector offers (the engine accepts up to 52; older weeks are rarely worth a click). */
export const WEEK_CHOICES = 12;

/** "12 Oct to 19 Oct" for a week. */
export function weekRange(r: Pick<WeeklyReport, 'from' | 'to'>): string {
  return `${formatDate(r.from, r.to)} to ${formatDate(r.to, r.to)}`;
}

/** The choices of the week selector: this week is "Last 7 days"; earlier ones are named by the day they end. */
export function weekOptions(latestEnd: number | null, count = WEEK_CHOICES): { value: number; label: string }[] {
  return Array.from({ length: count }, (_, i) => {
    if (i === 0) return { value: 0, label: 'Last 7 days' };
    if (latestEnd === null) return { value: i, label: i === 1 ? 'The week before' : `${i} weeks ago` };
    const end = latestEnd - i * WEEK_MS;
    return { value: i, label: `Week ending ${formatDate(end, latestEnd)}` };
  });
}

/** A signed score change: "+3 vs the week before", "no change", "" when there is nothing to compare. */
export function scoreChangeText(r: Pick<WeeklyReport, 'score' | 'scoreChange'>): string {
  if (r.score === null) return '';
  if (r.scoreChange === 0) return 'no change against the week before';
  return `${r.scoreChange > 0 ? '+' : '−'}${Math.abs(r.scoreChange)} against the week before`;
}

/** A disk's free space line parts. */
export function diskForecast(d: ReportDisk): string {
  return forecastText(d.forecast);
}

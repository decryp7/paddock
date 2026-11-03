import type { ListSource, ListStatus, SubscriptionStatus } from '../../core/api.types';

/** "12,400 entries · 300 duplicates removed · 40 covered by a wider range" (parts that are zero are left out). */
export function countsLine(c: SubscriptionStatus): string {
  const n = (v: number) => v.toLocaleString('en-US');
  const parts = [`${n(c.entries)} ${c.entries === 1 ? 'entry' : 'entries'}`];
  if (c.duplicates > 0) parts.push(`${n(c.duplicates)} ${c.duplicates === 1 ? 'duplicate' : 'duplicates'} removed`);
  if (c.covered > 0) parts.push(`${n(c.covered)} covered by a wider range`);
  if (c.skipped > 0) parts.push(`${n(c.skipped)} ${c.skipped === 1 ? 'line' : 'lines'} not taken`);
  return parts.join(' · ');
}

/** What kind of list this is, in words. */
export function sourceText(s: ListSource): string {
  if (s === 'manual') return 'Typed by you';
  if ('file' in s) return `Imported from ${s.file.name}`;
  return 'Subscription';
}

/** The address of a subscription, or null. */
export function urlOf(s: ListSource): string | null {
  return typeof s === 'object' && 'url' in s ? s.url.url : null;
}

/** Hours between automatic updates of a subscription, or null when it updates only when asked. */
export function everyHours(s: ListSource): number | null {
  return typeof s === 'object' && 'url' in s && typeof s.url.schedule === 'object' ? s.url.schedule.automatic.every : null;
}

/** The state line of a subscription: updating, error, note, or when it was last taken. */
export function updateLine(l: ListStatus, updating: boolean, now: number): string {
  if (updating) return 'Updating…';
  if (l.counts.error) return l.counts.error;
  if (l.counts.note) return l.counts.note;
  if (l.counts.lastFetchedMs === null) return 'Not fetched yet.';
  const mins = Math.max(0, Math.round((now - l.counts.lastFetchedMs) / 60_000));
  const ago = mins < 1 ? 'just now' : mins < 60 ? `${mins} min ago` : mins < 1440 ? `${Math.round(mins / 60)} h ago` : `${Math.round(mins / 1440)} d ago`;
  return `Last updated ${ago}.`;
}

/** Why "Update now" is not available yet, or null when it is. */
export function updateBlockedReason(l: ListStatus, now: number): string | null {
  if (l.nextAllowedMs !== null && l.nextAllowedMs > now) {
    return `Next update allowed at ${new Date(l.nextAllowedMs).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })} (a list is fetched at most once an hour).`;
  }
  return null;
}

import type { Notice } from './wire.types';

// A message's words (pure): split from the panel so the entry exports them without loading it (plan S16).

/** "Export failed (×4)" when it happened more than once. */
export function titleWithCount(n: Pick<Notice, 'title' | 'count'>): string {
  return n.count > 1 ? `${n.title} (×${n.count})` : n.title;
}

/** "14:05" today, else "24 Oct, 14:05". */
export function noticeTime(at: number, now: number): string {
  const d = new Date(at);
  const time = d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
  return new Date(now).toDateString() === d.toDateString() ? time : `${d.toLocaleDateString('en-GB', { day: 'numeric', month: 'short' })}, ${time}`;
}

/** What the status item says: the unread count when there is one. */
export function notificationsStatusText(unread: number): string {
  return unread > 0 ? $localize`:@@pd.notifications.status.unread:${unread}:count: unread` : $localize`:@@pd.notifications.status.none:Notifications`;
}

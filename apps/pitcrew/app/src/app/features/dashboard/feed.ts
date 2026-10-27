import type { ActivityReply, PitcrewNotification } from '../../core/api.types';
import { eventLook, isFeedEvent, notificationIcon, severityTone, type Tone } from '../../shared/labels';

/** One line of the events feed. */
export interface FeedItem {
  key: string;
  at: number;
  runner: string;
  tone: Tone;
  icon: string;
  title: string;
  detail: string;
}

/**
 * The events feed: the runners' history events (restarts and why, updates, cleanups) and the service's notifications
 * (e.g. an unreachable server), newest first. Display shaping only.
 */
export function feedItems(activity: Record<string, ActivityReply | null>, notifications: PitcrewNotification[], limit = 40): FeedItem[] {
  const items: FeedItem[] = [];
  for (const [runner, reply] of Object.entries(activity)) {
    for (const e of reply?.events ?? []) {
      if (!isFeedEvent(e)) continue;
      const look = eventLook(e);
      items.push({ key: `${runner}-${e.kind}-${e.at}`, at: e.at, runner, tone: look.tone, icon: look.icon, title: look.title, detail: look.detail });
    }
  }
  for (const n of notifications) {
    items.push({ key: `n-${n.runner}-${n.kind}-${n.at}`, at: n.at, runner: n.runner, tone: severityTone(n.severity), icon: notificationIcon(n.kind), title: n.title, detail: n.body });
  }
  return items.sort((a, b) => b.at - a.at).slice(0, limit);
}

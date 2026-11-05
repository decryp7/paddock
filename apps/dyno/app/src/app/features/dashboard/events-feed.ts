import { FullTextTip } from '../../shared/full-text-tip';
import { CdkVirtualScrollViewport, CdkFixedSizeVirtualScroll, CdkVirtualForOf } from '@angular/cdk/scrolling';
import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import type { ActivityEntry, Alert } from '../../core/api.types';
import { formatRelative } from '../../shared/format';
import { Icon } from '../../shared/icon';
import type { IconName } from '../../shared/icons';
import { ACTIVITY_ICON, ALERT_ICON, SEVERITY_TONE, type Tone } from '../../shared/labels';
import { ActivityStore } from '../../state/activity.store';
import { AlertsStore } from '../../state/alerts.store';
import { LiveStore } from '../../state/live.store';

export interface FeedItem {
  id: string;
  at: number;
  icon: IconName;
  tone: Tone;
  title: string;
  detail: string;
}

/** Merges activity and alerts into one feed, newest first (pure). */
export function buildFeed(activity: readonly ActivityEntry[], alerts: readonly Alert[], limit: number): FeedItem[] {
  const fromActivity = activity.map<FeedItem>((a) => ({
    id: `a-${a.id}`,
    at: a.at,
    icon: ACTIVITY_ICON[a.kind],
    tone: a.kind === 'alert' ? 'warn' : 'muted',
    title: a.title,
    detail: a.detail,
  }));
  const fromAlerts = alerts.map<FeedItem>((a) => ({ id: `n-${a.id}`, at: a.at, icon: ALERT_ICON[a.kind], tone: SEVERITY_TONE[a.severity], title: a.title, detail: a.detail }));
  return [...fromActivity, ...fromAlerts].sort((x, y) => y.at - x.at).slice(0, limit);
}

/**
 * How many entries the board asks for of each kind (activity and alerts). The engine answers at most 500 of each; asking for fewer keeps
 * the board's first read small. When either list reaches this, the feed says it shows the latest ones only.
 */
export const EVENTS_LOADED = 200;
/** The height of one entry (two lines and its padding), in px: the virtual viewport's fixed item size. */
export const EVENT_ROW_PX = 48;

/** What Dyno noticed and what it warned about, newest first. */
@Component({
  selector: 'dy-events-feed',
  imports: [FullTextTip, Icon, CdkVirtualScrollViewport, CdkFixedSizeVirtualScroll, CdkVirtualForOf],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <cdk-virtual-scroll-viewport class="viewport" [itemSize]="rowPx" role="list" tabindex="0" aria-label="Activity and alerts" [style.--n]="items().length">
      <div *cdkVirtualFor="let i of items(); trackBy: trackId" class="row" role="listitem">
        <span class="ic" [attr.data-tone]="i.tone"><dy-icon [name]="i.icon" /></span>
        <div class="txt">
          <span class="title" [dyFullText]="i.title">{{ i.title }}</span>
          <span class="detail truncate" [dyFullText]="i.detail">{{ i.detail }}</span>
        </div>
        <span class="when num">{{ ago(i.at) }}</span>
      </div>
    </cdk-virtual-scroll-viewport>
    @if (capped()) {
      <p class="more num">Showing the latest {{ items().length }} events.</p>
    }
  `,
  styles: `
    :host { display: block; min-width: 0; }
    /* As tall as its entries, up to the widget's body (--wv-avail); a longer list scrolls inside it. The wheel passes on at the ends. */
    .viewport { height: min(calc(var(--n, 1) * 48px), var(--wv-avail, 336px)); min-height: 48px; }
    .viewport:focus-visible { outline-offset: -2px; }
    .row { box-sizing: border-box; height: 48px; display: flex; align-items: center; gap: var(--sp-3); padding: 0 var(--sp-1); border-radius: var(--r-md); }
    .row:hover { background: var(--surface-hover); }
    .ic { display: grid; place-items: center; width: 28px; height: 28px; border-radius: var(--r-md); background: var(--surface-active); color: var(--text-2); flex: none; }
    .ic dy-icon { --icon-size: 15px; }
    .ic[data-tone='warn'] { background: var(--warn-soft); color: var(--warn); }
    .ic[data-tone='bad'] { background: var(--bad-soft); color: var(--bad); }
    .ic[data-tone='info'] { background: var(--info-soft); color: var(--info); }
    .txt { flex: 1; min-width: 0; display: grid; }
    .title { font-weight: var(--fw-medium); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .detail { color: var(--text-3); font-size: var(--fs-xs); }
    .more { margin: 0; padding: var(--sp-1); color: var(--text-3); font-size: var(--fs-xs); }
    .when { color: var(--text-3); font-size: var(--fs-xs); white-space: nowrap; }
  `,
})
export class EventsFeed {
  protected readonly rowPx = EVENT_ROW_PX;
  protected readonly trackId = (_: number, i: FeedItem) => i.id;
  protected readonly activity = inject(ActivityStore);
  protected readonly alerts = inject(AlertsStore);
  private readonly live = inject(LiveStore);
  /** Every entry the stores hold, newest first. */
  protected readonly items = computed(() => buildFeed(this.activity.entries(), this.alerts.alerts(), Infinity));
  /** Either list is as long as the board asked for, so older entries exist that the feed does not show. */
  protected readonly capped = computed(() => this.activity.entries().length >= EVENTS_LOADED || this.alerts.alerts().length >= EVENTS_LOADED);
  /** Relative to the engine's clock, so the feed reads right whatever the computer's clock does. */
  protected readonly ago = (at: number) => formatRelative(at, this.live.system()?.at ?? Date.now());
}

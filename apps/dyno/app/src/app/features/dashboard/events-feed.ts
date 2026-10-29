import { FullTextTip } from '../../shared/full-text-tip';
import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import type { ActivityEntry, Alert } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
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

/** What Dyno noticed and what it warned about, newest first. */
@Component({
  selector: 'dy-events-feed',
  imports: [FullTextTip, AsyncState, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card" aria-label="Activity and alerts">
      <div class="card-head">
        <h2><dy-icon name="bell" /> Activity and alerts</h2>
      </div>
      <dy-async [loaded]="loaded()" [error]="error()" [empty]="items().length === 0" emptyTitle="Nothing has happened yet" emptyText="Alerts and notes from Dyno itself show up here. Dyno only looks: it changes nothing on your computer." what="activity" [rows]="4" (retry)="reload()">
        <ul>
          @for (i of items(); track i.id) {
            <li>
              <span class="ic" [attr.data-tone]="i.tone"><dy-icon [name]="i.icon" /></span>
              <div class="txt">
                <span class="title" [dyFullText]="i.title">{{ i.title }}</span>
                <span class="detail truncate" [dyFullText]="i.detail">{{ i.detail }}</span>
              </div>
              <span class="when num">{{ ago(i.at) }}</span>
            </li>
          }
        </ul>
      </dy-async>
    </section>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .card { height: 100%; padding-bottom: var(--sp-2); }
    dy-async { padding: 0 var(--sp-2); }
    ul { list-style: none; margin: 0; padding: var(--sp-1) 0 0; display: grid; }
    li { display: flex; align-items: center; gap: var(--sp-3); padding: 7px var(--sp-2); border-radius: var(--r-md); }
    li:hover { background: var(--surface-hover); }
    .ic { display: grid; place-items: center; width: 28px; height: 28px; border-radius: var(--r-md); background: var(--surface-active); color: var(--text-2); flex: none; }
    .ic dy-icon { --icon-size: 15px; }
    .ic[data-tone='warn'] { background: var(--warn-soft); color: var(--warn); }
    .ic[data-tone='bad'] { background: var(--bad-soft); color: var(--bad); }
    .ic[data-tone='info'] { background: var(--info-soft); color: var(--info); }
    .txt { flex: 1; min-width: 0; display: grid; }
    .title { font-weight: var(--fw-medium); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .detail { color: var(--text-3); font-size: var(--fs-xs); }
    .when { color: var(--text-3); font-size: var(--fs-xs); white-space: nowrap; }
  `,
})
export class EventsFeed implements OnInit {
  protected readonly activity = inject(ActivityStore);
  protected readonly alerts = inject(AlertsStore);
  private readonly live = inject(LiveStore);
  protected readonly items = computed(() => buildFeed(this.activity.entries(), this.alerts.alerts(), 7));
  protected readonly loaded = computed(() => this.activity.loaded() || this.alerts.loaded());
  protected readonly error = computed(() => (this.activity.loaded() || this.alerts.loaded() ? null : (this.activity.error() ?? this.alerts.error())));
  /** Relative to the engine's clock, so the feed reads right whatever the PC's clock does. */
  protected readonly ago = (at: number) => formatRelative(at, this.live.system()?.at ?? Date.now());

  ngOnInit(): void {
    this.reload();
  }

  protected reload(): void {
    void this.activity.load(30);
    void this.alerts.load(20);
  }
}

import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { RouterLink } from '@angular/router';
import { SuEmptyState } from '@decryp7/paddock-ui';
import { formatBytes } from '../../shared/format';
import { eventView } from '../../shared/labels';
import { AgoShortPipe } from '../../shared/ui/ago.pipe';
import { Panel } from '../../shared/ui/panel';
import { DashboardStore } from '../../state/dashboard.store';
import type { AppEvent } from '../../core/api.types';

/** The activity feed: new releases, failed checks, recoveries, downloads. "Load more" pages back in time. */
@Component({
  selector: 'sp-events-feed',
  imports: [MatIcon, RouterLink, SuEmptyState, Panel, AgoShortPipe],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <sp-panel [state]="store.events()" label="the activity feed" (retry)="store.loadEvents()">
      @let events = store.events().value ?? [];
      @if (events.length) {
        <ul class="list">
          @for (e of events; track e.atMs + e.kind + (e.watchId ?? '') + (e.version ?? '')) {
            @let v = view(e);
            <li class="item">
              <mat-icon class="ico" [class]="v.tone" [svgIcon]="'sp:' + v.icon" aria-hidden="true" />
              <div class="main truncate">
                <span class="what">{{ v.title }}</span>
                @if (e.watchId) {
                  <a [routerLink]="['/watches', e.watchId]">{{ e.watchName }}</a>
                }
                <span class="detail">{{ detail(e) }}</span>
              </div>
              <time class="when">{{ e.atMs | agoShort }}</time>
            </li>
          }
        </ul>
        @if (store.moreEvents()) {
          <button type="button" class="btn ghost more" (click)="store.loadMoreEvents()">Load more</button>
        }
      } @else {
        <pd-empty-state heading="Nothing has happened yet" description="Releases, failed checks and downloads are listed here." />
      }
    </sp-panel>
  `,
  styles: `
    .list {
      margin: 0 0 var(--pd-space-3);
      padding: 0;
      list-style: none;
    }
    .item {
      display: flex;
      align-items: center;
      gap: var(--pd-space-3);
      min-height: 36px;
      padding: var(--pd-space-1) var(--pd-space-3);
      border-bottom: 1px solid var(--pd-hairline);
    }
    .item:last-child {
      border-bottom: 0;
    }
    .ico {
      flex: none;
      width: 16px;
      height: 16px;
      color: var(--pd-text-subtle);
    }
    .ico.danger { color: var(--pd-danger); }
    .ico.warning { color: var(--pd-warning); }
    .ico.success { color: var(--pd-success); }
    .main {
      flex: 1;
      min-width: 0;
    }
    .what {
      margin-right: var(--pd-space-2);
      font-weight: var(--pd-weight-medium);
    }
    a {
      color: var(--pd-text);
      text-decoration: none;
    }
    a:hover {
      text-decoration: underline;
    }
    .detail {
      margin-left: var(--pd-space-2);
      color: var(--pd-text-muted);
    }
    .when {
      color: var(--pd-text-subtle);
      font-size: var(--pd-text-caption);
      white-space: nowrap;
    }
  `,
})
export class EventsFeed {
  protected readonly store = inject(DashboardStore);

  protected view(e: AppEvent) {
    return eventView(e.kind);
  }

  /** What the event says besides who it is about. */
  protected detail(e: AppEvent): string {
    switch (e.kind) {
      case 'new-release':
        return e.version ?? '';
      case 'download-finished':
        return `${e.version ?? ''} (${formatBytes(e.size)})${e.verified ? ', checksum verified' : ''}`;
      default:
        return e.message ?? '';
    }
  }
}

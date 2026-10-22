import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { RouterLink } from '@angular/router';
import { SuEmptyState, SuStatusPill } from '@decryp7/paddock-ui';
import { formatBytes } from '../../shared/format';
import { eventView } from '../../shared/labels';
import { AgoPipe } from '../../shared/ui/ago.pipe';
import { Panel } from '../../shared/ui/panel';
import { DashboardStore } from '../../state/dashboard.store';
import type { AppEvent } from '../../core/api.types';

/** The activity feed: new releases, failed checks, recoveries, downloads. "Load more" pages back in time. */
@Component({
  selector: 'sp-events-feed',
  imports: [RouterLink, SuEmptyState, SuStatusPill, Panel, AgoPipe],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <sp-panel [state]="store.events()" label="the activity feed" (retry)="store.loadEvents()">
      @let events = store.events().value ?? [];
      @if (events.length) {
        <ul class="list">
          @for (e of events; track e.atMs + e.kind + (e.watchId ?? '') + (e.version ?? '')) {
            @let v = view(e);
            <li class="item">
              <pd-status-pill [status]="v.tone" [label]="v.title" />
              <div class="main truncate">
                @if (e.watchId) {
                  <a [routerLink]="['/watches', e.watchId]">{{ e.watchName }}</a>
                }
                <span class="detail">{{ detail(e) }}</span>
              </div>
              <time class="when">{{ e.atMs | ago }}</time>
            </li>
          }
        </ul>
        @if (store.moreEvents()) {
          <button type="button" class="btn more" (click)="store.loadMoreEvents()">Load more</button>
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
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-card);
      background: var(--pd-surface-1);
    }
    .item {
      display: grid;
      grid-template-columns: 150px 1fr auto;
      align-items: center;
      gap: var(--pd-space-3);
      padding: var(--pd-space-2) var(--pd-space-4);
      border-bottom: 1px solid var(--pd-hairline);
    }
    .item:last-child {
      border-bottom: 0;
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
    @media (max-width: 720px) {
      .item {
        grid-template-columns: 1fr auto;
      }
      pd-status-pill {
        grid-column: 1 / -1;
      }
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

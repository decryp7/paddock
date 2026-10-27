import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { formatRelative } from '../../shared/format';
import type { FeedItem } from './feed';

/** The events feed list. */
@Component({
  selector: 'pc-events-feed',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (items().length === 0) {
      <p class="none faint">Nothing happened in this range: no restarts, updates or cleanups.</p>
    } @else {
      <ol class="feed">
        @for (it of items(); track it.key) {
          <li>
            <span class="icon" [class]="it.tone"><mat-icon [svgIcon]="'pc:' + it.icon" aria-hidden="true" /></span>
            <div class="text">
              <div class="line">
                <span class="title">{{ it.title }}</span>
              </div>
              <div class="detail muted">{{ it.detail }}</div>
              <div class="meta faint">
                <span class="runner">{{ it.runner }}</span> · <time [attr.datetime]="iso(it.at)">{{ ago(it.at) }}</time>
              </div>
            </div>
          </li>
        }
      </ol>
    }
  `,
  styles: `
    :host {
      display: block;
      min-height: 0;
    }
    .feed {
      list-style: none;
      margin: 0;
      padding: 0;
      display: grid;
    }
    li {
      display: flex;
      gap: var(--space-3);
      padding: var(--space-2) 0;
      border-bottom: 1px solid var(--border);
    }
    li:last-child {
      border-bottom: 0;
    }
    .icon {
      flex: none;
      display: grid;
      place-items: center;
      width: 26px;
      height: 26px;
      border-radius: var(--radius-full);
      background: var(--neutral-soft);
      color: var(--neutral);
    }
    .icon mat-icon {
      width: 14px;
      height: 14px;
    }
    .icon.success {
      background: var(--success-soft);
      color: var(--success);
    }
    .icon.info {
      background: var(--info-soft);
      color: var(--info);
    }
    .icon.warning {
      background: var(--warning-soft);
      color: var(--warning);
    }
    .icon.danger {
      background: var(--danger-soft);
      color: var(--danger);
    }
    .text {
      min-width: 0;
      display: grid;
      gap: 1px;
    }
    .title {
      font-weight: var(--weight-medium);
    }
    .detail {
      font-size: var(--text-sm);
      overflow-wrap: anywhere;
    }
    .meta {
      font-size: var(--text-xs);
    }
    .runner {
      font-weight: var(--weight-medium);
      color: var(--text-2);
    }
    .none {
      padding: var(--space-4) 0;
      font-size: var(--text-sm);
    }
  `,
})
export class EventsFeed {
  readonly items = input.required<FeedItem[]>();
  readonly now = input.required<number>();

  protected ago(at: number): string {
    return formatRelative(at, this.now());
  }

  protected iso(at: number): string {
    return new Date(at).toISOString();
  }
}

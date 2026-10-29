import { ChangeDetectionStrategy, Component, inject, input } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { LiveStore } from '../../core/live.store';
import { ago } from '../../shared/format';
import { REASON_LABEL, eventIcon, severityTone } from '../../shared/labels';
import { EmptyState } from '../../shared/ui/empty-state';

/** What Pitwall did and why: loads and releases (with the reason), restarts, errors, downloads, pairings. */
@Component({
  selector: 'pw-events-feed',
  imports: [MatIconModule, EmptyState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (live.events().length === 0) {
      <pw-empty-state icon="activity" heading="No events yet" text="Loads, releases and errors show up here as they happen." />
    }
    <ol aria-label="Events" aria-live="off">
      @for (e of live.events().slice(0, limit()); track e.id) {
        <li [attr.data-tone]="tone(e.severity)">
          <span class="ic"><mat-icon [svgIcon]="'pw:' + icon(e.kind)" aria-hidden="true" /></span>
          <div class="main">
            <p class="msg pitwall-selectable">{{ e.message }}</p>
            <p class="meta">
              <time class="num">{{ when(e.at, now()) }}</time>
              @if (e.reason) {
                <span class="reason">{{ reason[e.reason] }}</span>
              }
            </p>
          </div>
        </li>
      }
    </ol>
  `,
  styles: `
    :host {
      display: block;
      min-height: 0;
      overflow: auto;
      margin: 0 calc(-1 * var(--pd-space-2));
      padding: 0 var(--pd-space-2);
      /* A list longer than its card fades out instead of showing a half item at the edge. */
      mask-image: linear-gradient(to bottom, #000 calc(100% - 28px), transparent);
    }
    ol {
      list-style: none;
      margin: 0;
      padding: 0;
    }
    li {
      position: relative;
      display: grid;
      grid-template-columns: 24px minmax(0, 1fr);
      gap: var(--pd-space-3);
      padding: 0 0 var(--pd-space-3);
      &:not(:last-child)::before {
        content: '';
        position: absolute;
        left: 11.5px;
        top: 26px;
        bottom: 2px;
        width: 1px;
        background: var(--pd-hairline);
      }
    }
    .ic {
      display: grid;
      place-items: center;
      width: 24px;
      height: 24px;
      border-radius: 50%;
      background: var(--pd-hover);
      color: var(--pd-text-muted);
      mat-icon {
        width: 13px;
        height: 13px;
      }
    }
    li[data-tone='warn'] .ic {
      background: var(--pd-warning-tint);
      color: var(--pd-warning);
    }
    li[data-tone='bad'] .ic {
      background: var(--pd-danger-tint);
      color: var(--pd-danger);
    }
    .msg {
      padding-top: 3px;
      font-size: var(--pd-text-label);
      line-height: 1.4;
    }
    .meta {
      display: flex;
      gap: var(--pd-space-2);
      margin-top: 2px;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .reason::before {
      content: '·';
      margin-right: var(--pd-space-2);
    }
  `,
})
export class EventsFeed {
  readonly now = input.required<number>();
  readonly limit = input(30);
  protected readonly live = inject(LiveStore);
  protected readonly icon = eventIcon;
  protected readonly tone = severityTone;
  protected readonly when = ago;
  protected readonly reason = REASON_LABEL;
}

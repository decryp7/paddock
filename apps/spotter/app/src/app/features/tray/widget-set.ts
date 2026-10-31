import { ChangeDetectionStrategy, Component, computed, inject, output, viewChildren } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { MatTooltip } from '@angular/material/tooltip';
import { PdSkeleton, PdWidget } from '@decryp7/paddock-ui/widgets';
import { ClockService } from '../../core/clock.service';
import type { PopoverState } from '../../core/widgets.types';
import { TrayWidgetsStore } from '../../state/tray-widgets.store';
import type { FailureKind, NotificationMode, PopoverValue, PopoverWidget, SiteState, WidgetActionId } from '../../core/generated/widgets';
import { formatShortDateTime } from '../../shared/format';
import { AgoShortPipe, UntilShortPipe } from '../../shared/ui/ago.pipe';
import { Version } from '../../shared/ui/version';

type Of<K extends PopoverValue['kind']> = Extract<PopoverValue, { kind: K }>;

/** The words and icons of a site's state: status is never colour alone, so every state has both. */
const SITE: Record<SiteState, { icon: string; word: string }> = {
  ok: { icon: 'sp:check', word: 'Up to date' },
  new: { icon: 'sp:sparkle', word: 'New release' },
  checking: { icon: 'sp:refresh', word: 'Checking' },
  paused: { icon: 'sp:pause', word: 'Paused' },
  attention: { icon: 'sp:alert', word: 'Needs attention' },
  pending: { icon: 'sp:clock', word: 'Not checked yet' },
};

const FAILURE: Record<FailureKind, string> = {
  unreachable: 'Unreachable',
  authNeeded: 'Sign-in needed',
  needsYou: 'Needs you',
  invalid: 'Set-up problem',
  failed: 'Check failed',
};

/**
 * Spotter's templates for the popup's widget kinds (family ADR 0035): one `ng-template` per kind, drawn by `pd-widget-area`
 * inside a `pd-widget-frame`, which owns the head and the loading, working, empty, error and stale states. The same set draws
 * the popup and the Settings page's preview. A template draws the engine's value and decides nothing; what a button does is
 * said as a [`WidgetActionId`] on `run`, and the host runs it.
 */
@Component({
  selector: 'sp-widget-set',
  imports: [MatIcon, MatTooltip, PdSkeleton, PdWidget, AgoShortPipe, UntilShortPipe, Version],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <ng-template pdWidget="watchedSites" [pdWidgetOf]="sites" let-v>
      <ul class="rows">
        @for (r of v?.rows ?? placeholders('watchedSites'); track $index) {
          <li>
            <button type="button" class="row" [disabled]="!r" [attr.aria-label]="r ? r.name + ', ' + site(r.state).word + ', open' : null" (click)="r && run.emit({ type: 'openWatch', id: r.id })">
              <mat-icon class="ic" [class]="'ic ' + (r ? 'st-' + r.state : '')" [svgIcon]="r ? site(r.state).icon : 'sp:watches'" aria-hidden="true" pdSkeleton />
              <span class="name truncate" pdSkeleton>{{ r?.name ?? 'Watch name' }}</span>
              <span class="meta truncate" pdSkeleton>
                {{ r ? site(r.state).word : 'Up to date' }}@if (r?.version; as ver) { · <sp-ver [value]="ver" /> }
              </span>
              <mat-icon class="chev" svgIcon="sp:chevron-right" aria-hidden="true" />
            </button>
          </li>
        }
      </ul>
      @if (v && v.more > 0) {
        <p class="more">+{{ v.more }} more in Spotter</p>
      }
    </ng-template>

    <ng-template pdWidget="recentChanges" [pdWidgetOf]="changes" let-v>
      <ul class="rows">
        @for (r of v?.rows ?? placeholders('recentChanges'); track $index) {
          <li>
            <button type="button" class="row" [disabled]="!r" [attr.aria-label]="r ? r.name + ' ' + r.version + (r.unseen ? ', new' : '') + ', open' : null" (click)="r && run.emit({ type: 'openWatch', id: r.id })">
              <mat-icon class="ic" [class]="'ic ' + (r?.unseen ? 'st-new' : '')" [svgIcon]="r?.unseen ? 'sp:sparkle' : 'sp:history'" aria-hidden="true" pdSkeleton />
              <span class="name truncate" pdSkeleton>{{ r?.name ?? 'Watch name' }}</span>
              <span class="meta truncate" pdSkeleton>
                @if (r) { <sp-ver [value]="r.version" /> · @if (r.unseen) {New · }{{ r.firstSeenMs | agoShort }} } @else { 1.2.0 · 3 h }
              </span>
              <mat-icon class="chev" svgIcon="sp:chevron-right" aria-hidden="true" />
            </button>
          </li>
        }
      </ul>
    </ng-template>

    <ng-template pdWidget="nextCheck" [pdWidgetOf]="next" let-v>
      <button type="button" class="block" [disabled]="!v" [attr.aria-label]="v ? 'Next check: ' + v.name + (v.due ? ' is due now' : ' in ' + (v.atMs | untilShort)) + ', open' : null" (click)="v && run.emit({ type: 'openWatch', id: v.id })">
        <span class="big" pdSkeleton>{{ v?.due ? 'Due now' : 'In ' + (v ? (v.atMs | untilShort) : '12 min') }}</span>
        <span class="meta truncate" pdSkeleton>{{ v?.name ?? 'Watch name' }}@if (v && v.alsoDue > 0) { · +{{ v.alsoDue }} more }</span>
        <mat-icon class="chev" svgIcon="sp:chevron-right" aria-hidden="true" />
      </button>
    </ng-template>

    <ng-template pdWidget="quietHours" [pdWidgetOf]="quiet" let-v>
      <div class="block">
        <span class="big" pdSkeleton>{{ v ? v.start + ' – ' + v.end : '22:00 – 07:00' }}</span>
        <span class="meta" pdSkeleton>
          <mat-icon class="ic-inline" [svgIcon]="v?.active ? 'sp:moon' : 'sp:sun'" aria-hidden="true" />{{ v?.active ? 'In force now' : 'Not now' }}
        </span>
      </div>
    </ng-template>

    <ng-template pdWidget="failures" [pdWidgetOf]="failures" let-v>
      <ul class="rows">
        @for (r of v?.rows ?? placeholders('failures'); track $index) {
          <li class="fail">
            <button type="button" class="row" [disabled]="!r" [attr.aria-label]="r ? r.name + ', ' + failure(r.kind) + ', open' : null" (click)="r && run.emit({ type: 'openWatch', id: r.id })">
              <mat-icon class="ic st-attention" svgIcon="sp:alert" aria-hidden="true" pdSkeleton />
              <span class="name truncate" pdSkeleton>{{ r?.name ?? 'Watch name' }}</span>
              <span class="meta truncate" [matTooltip]="r ? failure(r.kind) + ': ' + r.message : ''" pdSkeleton>{{ r ? failure(r.kind) + ': ' + r.message : 'Check failed: the place did not answer' }}</span>
            </button>
            <button type="button" class="retry" [disabled]="!r" [attr.aria-label]="r ? 'Retry check of ' + r.name : null" (click)="r && run.emit({ type: 'retryCheck', id: r.id })" pdSkeleton>
              <mat-icon class="ic-inline" svgIcon="sp:refresh" aria-hidden="true" />Retry
            </button>
          </li>
        }
      </ul>
    </ng-template>

    <ng-template pdWidget="notifications" [pdWidgetOf]="notices" let-v>
      <div class="block">
        <span class="big" pdSkeleton>
          <mat-icon class="ic-inline" [svgIcon]="!v || v.mode.kind === 'on' ? 'sp:bell' : 'sp:bell-off'" aria-hidden="true" />{{ v ? mode(v.mode) : 'On' }}
        </span>
        @if (v?.mode?.kind === 'paused') {
          <button type="button" class="link" (click)="run.emit({ type: 'resumeNotifications' })">Resume notifications</button>
        } @else if (v?.mode?.kind === 'off') {
          <button type="button" class="link" (click)="run.emit({ type: 'openSettings' })">Open Settings</button>
        } @else {
          <span class="meta" pdSkeleton>You will hear about new releases</span>
        }
      </div>
    </ng-template>
  `,
  styles: `
    :host {
      display: contents;
    }
    .rows {
      display: grid;
      margin: 0;
      padding: 0;
      list-style: none;
    }
    .row {
      display: grid;
      grid-template-columns: 16px minmax(0, 1fr) 14px;
      column-gap: var(--pd-space-2);
      align-items: center;
      min-height: 30px;
      /* The icon sits on the widget head's left edge; the hover tint reaches past it. */
      width: calc(100% + 2 * var(--pd-space-1));
      margin: 0 calc(-1 * var(--pd-space-1));
      padding: 1px var(--pd-space-1);
      border: 0;
      border-radius: var(--pd-radius-control);
      background: transparent;
      color: var(--pd-text);
      font: inherit;
      text-align: left;
    }
    .row:not(:disabled):hover {
      background: var(--pd-hover);
    }
    .row:focus-visible,
    .block:focus-visible,
    .retry:focus-visible,
    .link:focus-visible {
      outline: var(--pd-focus-width) solid var(--pd-focus-ring);
      outline-offset: 1px;
    }
    .ic {
      grid-row: 1 / span 2;
      width: 16px;
      height: 16px;
      color: var(--pd-text-muted);
    }
    .chev {
      grid-column: 3;
      grid-row: 1 / span 2;
      width: 14px;
      height: 14px;
      color: var(--pd-text-subtle);
    }
    button.block .chev {
      grid-column: 2;
      grid-row: 1 / span 2;
      align-self: center;
    }
    .name {
      font-weight: var(--pd-weight-medium);
    }
    .meta {
      grid-column: 2;
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
    }
    .st-new,
    .st-checking {
      color: var(--pd-info);
    }
    .st-attention {
      color: var(--pd-danger);
    }
    .st-ok {
      color: var(--pd-success);
    }
    .more {
      margin: 0;
      padding: var(--pd-space-1);
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
    }
    .block {
      display: grid;
      grid-template-columns: minmax(0, 1fr) auto;
      gap: 0 var(--pd-space-2);
      width: 100%;
      padding: 0;
      border: 0;
      border-radius: var(--pd-radius-control);
      background: transparent;
      color: var(--pd-text);
      font: inherit;
      text-align: left;
    }
    button.block:not(:disabled):hover {
      background: var(--pd-hover);
    }
    .block .meta {
      grid-column: 1;
    }
    .block .big {
      grid-column: 1;
    }
    .block > :not(.chev):not(.big):not(.meta) {
      grid-column: 1 / -1;
    }
    .big {
      font-size: var(--pd-text-title);
      font-weight: var(--pd-weight-semibold);
    }
    .ic-inline {
      width: 14px;
      height: 14px;
      margin-right: var(--pd-space-1);
      vertical-align: -2px;
    }
    .fail {
      display: grid;
      grid-template-columns: minmax(0, 1fr) auto;
      align-items: center;
    }
    .retry,
    .link {
      padding: var(--pd-space-1) var(--pd-space-2);
      border: 0;
      border-radius: var(--pd-radius-control);
      background: transparent;
      color: var(--pd-accent);
      font: inherit;
      font-weight: var(--pd-weight-medium);
    }
    .link {
      justify-self: start;
      margin-left: calc(-1 * var(--pd-space-2));
    }
    .retry:not(:disabled):hover,
    .link:hover {
      background: var(--pd-hover);
    }
    .truncate {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
  `,
})
export class WidgetSet {
  private readonly clock = inject(ClockService);
  /** What a button asks the app to do. */
  readonly run = output<WidgetActionId>();

  /** The kinds' templates, for `pd-widget-area`'s `templates` input (the popup and the preview share one set). */
  private readonly dirs = viewChildren(PdWidget);
  readonly widgets = computed(() => this.dirs() as readonly PdWidget<PopoverValue>[]);

  // Type witnesses for the templates' `let-v` (never read).
  /** The head's meta text of a ready widget ("3 failing", "8 new"): the engine's counts, worded. */
  readonly meta = (_id: string, state: PopoverState): string | null => {
    if (state.state !== 'ready') return null;
    const v = state.value;
    if (v.kind === 'failures') return `${v.rows.length + v.more} failing`;
    if (v.kind === 'recentChanges' && v.unseen > 0) return `${v.unseen} new`;
    return null;
  };

  protected readonly sites: Of<'watchedSites'> | null = null;
  protected readonly changes: Of<'recentChanges'> | null = null;
  protected readonly next: Of<'nextCheck'> | null = null;
  protected readonly quiet: Of<'quietHours'> | null = null;
  protected readonly failures: Of<'failures'> | null = null;
  protected readonly notices: Of<'notifications'> | null = null;
  private readonly store = inject(TrayWidgetsStore);

  /** The placeholder rows of a kind's loading twin: as many as the engine says the ready widget draws, so a popup does not change height. */
  protected placeholders(kind: PopoverWidget): null[] {
    const rows = this.store.shape().find((s) => s.kind === kind)?.rows ?? 0;
    return Array.from({ length: rows }, () => null);
  }

  protected site(s: SiteState): { icon: string; word: string } {
    return SITE[s];
  }

  protected failure(k: FailureKind): string {
    return FAILURE[k];
  }

  protected mode(m: NotificationMode): string {
    if (m.kind === 'paused') return `Paused until ${formatShortDateTime(m.untilMs, this.clock.now())}`;
    return m.kind === 'on' ? 'On' : 'Off';
  }
}

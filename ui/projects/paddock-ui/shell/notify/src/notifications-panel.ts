import { ChangeDetectionStrategy, Component, computed, effect, ElementRef, inject, signal, untracked } from '@angular/core';
import { SuEmptyState, SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { ContributionRegistry } from '@decryp7/paddock-ui/shell';
import { NoticeActions } from './notice-actions';
import { LOCAL_SOURCE, NotificationsStore, type ReadFilter } from './notifications.store';
import type { Notice, NoticeAction } from './wire.types';
import { noticeTime, titleWithCount } from './notice-text';


/** The headings of the shell's own sources (read when shown, so they are in the window's language). */
const BUILT_IN_SOURCES: Readonly<Record<string, () => string>> = {
  health: () => $localize`:@@pd.notifications.source.health:Services`,
  job: () => $localize`:@@pd.notifications.source.jobs:Jobs`,
  [LOCAL_SOURCE]: () => $localize`:@@pd.notifications.source.app:App`,
};


/**
 * The notification centre of the window: what happened and may need attention (the engine's messages, health outages,
 * the UI's own notices), newest first and grouped by source, with each message's actions, Mark read, Dismiss and Mark
 * all read. The engine keeps the list (dedupe with counts, caps, read state).
 */
@Component({
  selector: 'pd-notifications-panel',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SuEmptyState, SuSegmentedControl],
  template: `
    <div class="pd-bar">
      <pd-segmented-control [label]="t.filter" [options]="filters()" [value]="store.read()" (valueChange)="setRead($event)" />
      <span class="pd-spacer"></span>
      <button type="button" class="pd-btn pd-ghost" [disabled]="!store.unread()" data-testid="notices-mark-all" (click)="store.markAllRead()">{{ t.markAllRead }}</button>
    </div>
    @if (store.requestError(); as e) {
      <div class="pd-err" role="alert">
        <span class="pd-grow">{{ e }}</span>
        <button type="button" class="pd-btn" (click)="store.load()">{{ t.retry }}</button>
      </div>
    }
    @if (note(); as n) {
      <p class="pd-err" role="alert">{{ n }}</p>
    }
    @if (!store.notices().length) {
      <pd-empty-state data-testid="notices-empty" [heading]="t.emptyHeading" [description]="t.emptyDescription" />
    }
    @for (g of store.groups(); track g.source) {
      <h3 [id]="headingId(g.source)">{{ sourceLabel(g.source) }}</h3>
      <ul [attr.aria-labelledby]="headingId(g.source)">
        @for (n of g.notices; track n.id) {
          <li class="pd-notice" [class.pd-unread]="!n.read" [class.pd-focus]="store.focus() === n.id" [attr.data-severity]="n.severity" [attr.data-testid]="'notice-' + n.id" [attr.data-cause]="n.cause">
            <div class="pd-row">
              <span class="pd-dot" aria-hidden="true"></span>
              <strong class="pd-title">{{ titleWithCount(n) }}</strong>
              @if (!n.read) {
                <span class="pd-sr-only">{{ t.unreadMark }}</span>
              }
              <span class="pd-time">{{ time(n.at) }}</span>
              <span class="pd-row-actions">
                @if (!n.read) {
                  <button type="button" class="pd-icon-btn" [attr.aria-label]="markReadLabel(n.title)" [attr.data-testid]="'notice-read-' + n.id" (click)="store.markRead(n.id)">
                    <svg viewBox="0 0 14 14" width="14" height="14" aria-hidden="true" focusable="false"><path d="M2.5 7.5l3 3 6-7" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
                  </button>
                }
                <button type="button" class="pd-icon-btn" [attr.aria-label]="dismissLabel(n.title)" [attr.data-testid]="'notice-dismiss-' + n.id" (click)="store.dismiss(n.id)">
                  <svg viewBox="0 0 14 14" width="14" height="14" aria-hidden="true" focusable="false"><path d="M3.5 3.5l7 7M10.5 3.5l-7 7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
                </button>
              </span>
            </div>
            @if (n.body) {
              <p class="pd-body">{{ n.body }}</p>
            }
            @if (buttons(n).length) {
              <div class="pd-actions">
                @for (a of buttons(n); track $index) {
                  <button type="button" class="pd-btn" [attr.data-testid]="'notice-action-' + n.id + '-' + $index" (click)="act(n, a)">{{ actions.label(a, n) }}</button>
                }
              </div>
            }
          </li>
        }
      </ul>
    }
  `,
  styles: `
    :host { position: relative; display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; font-size: var(--pd-text-body); }
    .pd-bar { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-1); }
    .pd-spacer, .pd-grow { flex: 1; }
    .pd-err { display: flex; align-items: center; gap: var(--pd-space-2); padding: var(--pd-space-1) var(--pd-space-2); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-danger); margin: 0; }
    h3 { margin: var(--pd-space-2) 0 0; font-size: var(--pd-text-caption); font-weight: var(--pd-weight-semibold); color: var(--pd-text-muted); }
    ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--pd-space-1); }
    .pd-notice { border: 1px solid var(--pd-hairline); border-inline-start: 3px solid var(--pd-border-strong); border-radius: var(--pd-radius-control); padding: var(--pd-space-1) var(--pd-space-2); display: flex; flex-direction: column; gap: var(--pd-space-1); }
    .pd-notice[data-severity='error'], .pd-notice[data-severity='critical'] { border-inline-start-color: var(--pd-danger); }
    .pd-notice[data-severity='warning'] { border-inline-start-color: var(--pd-warning); }
    .pd-notice[data-severity='success'] { border-inline-start-color: var(--pd-success); }
    .pd-notice.pd-focus { outline: 2px solid var(--pd-focus-ring); }
    .pd-row { display: flex; align-items: center; gap: var(--pd-space-1); min-width: 0; }
    .pd-dot { width: 7px; height: 7px; border-radius: 50%; flex: none; }
    .pd-unread .pd-dot { background: var(--pd-accent); }
    .pd-title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: var(--pd-weight-medium); }
    .pd-notice:not(.pd-unread) .pd-title { font-weight: var(--pd-weight-regular); color: var(--pd-text-muted); }
    .pd-time { white-space: nowrap; color: var(--pd-text-muted); font-size: var(--pd-text-caption); font-variant-numeric: tabular-nums; }
    .pd-body { margin: 0; color: var(--pd-text-muted); overflow-wrap: anywhere; user-select: text; }
    .pd-actions { display: flex; flex-wrap: wrap; gap: var(--pd-space-1); justify-content: flex-end; }
    .pd-row-actions { display: flex; gap: 2px; opacity: 0; }
    .pd-notice:hover .pd-row-actions, .pd-notice:focus-within .pd-row-actions { opacity: 1; }
    @media (hover: none) { .pd-row-actions { opacity: 1; } }
    .pd-btn { min-height: 26px; padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; font-size: var(--pd-text-caption); font-weight: var(--pd-weight-medium); cursor: pointer; }
    .pd-btn.pd-ghost { background: transparent; border-color: transparent; color: var(--pd-accent); }
    .pd-btn:disabled { color: var(--pd-text-subtle); cursor: default; }
    .pd-icon-btn { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; border: 0; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text-muted); cursor: pointer; }
    .pd-icon-btn:hover { background: var(--pd-hover); color: var(--pd-text); }
    .pd-btn:focus-visible, .pd-icon-btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .pd-sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  `,
})
export class PdNotificationsPanel {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    filter: $localize`:@@pd.notifications.filter:Show`,
    emptyHeading: $localize`:@@pd.notifications.empty.heading:Nothing needs your attention`,
    emptyDescription: $localize`:@@pd.notifications.empty.description:Finished work, alerts and problems show up here.`,
    markAllRead: $localize`:@@pd.notifications.mark-all-read:Mark all read`,
    retry: $localize`:@@pd.notifications.retry:Retry`,
    unreadMark: $localize`:@@pd.notifications.unread-mark:, unread`,
  };
  protected readonly store = inject(NotificationsStore);
  protected readonly actions = inject(NoticeActions);
  private readonly registry = inject(ContributionRegistry, { optional: true });
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  protected readonly note = signal<string | null>(null);
  protected readonly titleWithCount = titleWithCount;
  protected readonly filters = computed<SuSegment[]>(() => [
    { value: 'all', label: $localize`:@@pd.notifications.filter.all:All` },
    { value: 'unread', label: $localize`:@@pd.notifications.filter.unread:Unread`, count: this.store.unread() },
  ]);

  constructor() {
    // A clicked OS notification or "Show in Notifications": scroll the message into view and focus its first button.
    effect(() => {
      const id = this.store.focus();
      if (id === null) return;
      untracked(() =>
        queueMicrotask(() => {
          const el = this.host.nativeElement.querySelector<HTMLElement>(`[data-testid="notice-${id}"]`);
          el?.scrollIntoView?.({ block: 'nearest' });
          el?.querySelector<HTMLElement>('button')?.focus();
        }),
      );
    });
  }

  protected setRead(value: string): void {
    this.store.setRead(value as ReadFilter);
  }

  protected headingId(source: string): string {
    return `pd-notices-${source.replace(/[^a-zA-Z0-9_-]/g, '-')}`;
  }

  /** A contributed notification kind's title, the shell's own sources, or the source id. */
  protected sourceLabel(source: string): string {
    const kind = this.registry?.notificationKinds().find((k) => k.id === source);
    return kind?.title.text ?? BUILT_IN_SOURCES[source]?.() ?? source;
  }

  protected markReadLabel(title: string): string {
    return $localize`:@@pd.notifications.mark-read:Mark read: ${title}:title:`;
  }

  protected dismissLabel(title: string): string {
    return $localize`:@@pd.notifications.dismiss:Dismiss ${title}:title:`;
  }

  protected time(at: number): string {
    return noticeTime(at, Date.now());
  }

  /** The message's buttons this window can run. */
  protected buttons(n: Notice): NoticeAction[] {
    return [n.primary, n.secondary, n.tertiary].filter((a): a is NoticeAction => a !== null && this.actions.available(a));
  }

  protected async act(n: Notice, a: NoticeAction): Promise<void> {
    this.note.set(await this.actions.run(a, n));
  }
}

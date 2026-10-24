import { ChangeDetectionStrategy, Component, computed, effect, ElementRef, inject, signal, untracked } from '@angular/core';
import { SuEmptyState, SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { ContributionRegistry } from '@decryp7/paddock-ui/shell';
import { NoticeActions } from './notice-actions';
import { LOCAL_SOURCE, NotificationsStore, type ReadFilter } from './notifications.store';
import type { Notice, NoticeAction } from './wire.types';

/** "Export failed (×4)" when it happened more than once. */
export function titleWithCount(n: Pick<Notice, 'title' | 'count'>): string {
  return n.count > 1 ? `${n.title} (×${n.count})` : n.title;
}

const BUILT_IN_SOURCES: Readonly<Record<string, string>> = { health: 'Services', job: 'Jobs', [LOCAL_SOURCE]: 'App' };

/** "14:05" today, else "24 Oct, 14:05". */
export function noticeTime(at: number, now: number): string {
  const d = new Date(at);
  const time = d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
  return new Date(now).toDateString() === d.toDateString() ? time : `${d.toLocaleDateString('en-GB', { day: 'numeric', month: 'short' })}, ${time}`;
}

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
    <div class="bar">
      <pd-segmented-control label="Show" [options]="filters()" [value]="store.read()" (valueChange)="setRead($event)" />
      <span class="spacer"></span>
      <button type="button" class="btn ghost" [disabled]="!store.unread()" data-testid="notices-mark-all" (click)="store.markAllRead()">Mark all read</button>
    </div>
    @if (store.requestError(); as e) {
      <div class="err" role="alert">
        <span class="grow">{{ e }}</span>
        <button type="button" class="btn" (click)="store.load()">Retry</button>
      </div>
    }
    @if (note(); as n) {
      <p class="err" role="alert">{{ n }}</p>
    }
    @if (!store.notices().length) {
      <pd-empty-state data-testid="notices-empty" heading="Nothing needs your attention" description="Finished work, alerts and problems show up here." />
    }
    @for (g of store.groups(); track g.source) {
      <h3 [id]="headingId(g.source)">{{ sourceLabel(g.source) }}</h3>
      <ul [attr.aria-labelledby]="headingId(g.source)">
        @for (n of g.notices; track n.id) {
          <li class="notice" [class.unread]="!n.read" [class.focus]="store.focus() === n.id" [attr.data-severity]="n.severity" [attr.data-testid]="'notice-' + n.id" [attr.data-cause]="n.cause">
            <div class="row">
              <span class="dot" aria-hidden="true"></span>
              <strong class="title">{{ titleWithCount(n) }}</strong>
              @if (!n.read) {
                <span class="sr-only">, unread</span>
              }
              <span class="time">{{ time(n.at) }}</span>
              <span class="row-actions">
                @if (!n.read) {
                  <button type="button" class="icon-btn" [attr.aria-label]="'Mark read: ' + n.title" [attr.data-testid]="'notice-read-' + n.id" (click)="store.markRead(n.id)">
                    <svg viewBox="0 0 14 14" width="14" height="14" aria-hidden="true" focusable="false"><path d="M2.5 7.5l3 3 6-7" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
                  </button>
                }
                <button type="button" class="icon-btn" [attr.aria-label]="'Dismiss ' + n.title" [attr.data-testid]="'notice-dismiss-' + n.id" (click)="store.dismiss(n.id)">
                  <svg viewBox="0 0 14 14" width="14" height="14" aria-hidden="true" focusable="false"><path d="M3.5 3.5l7 7M10.5 3.5l-7 7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
                </button>
              </span>
            </div>
            @if (n.body) {
              <p class="body">{{ n.body }}</p>
            }
            @if (buttons(n).length) {
              <div class="actions">
                @for (a of buttons(n); track $index) {
                  <button type="button" class="btn" [attr.data-testid]="'notice-action-' + n.id + '-' + $index" (click)="act(n, a)">{{ actions.label(a, n) }}</button>
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
    .bar { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-1); }
    .spacer, .grow { flex: 1; }
    .err { display: flex; align-items: center; gap: var(--pd-space-2); padding: var(--pd-space-1) var(--pd-space-2); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-danger); margin: 0; }
    h3 { margin: var(--pd-space-2) 0 0; font-size: var(--pd-text-caption); font-weight: var(--pd-weight-semibold); color: var(--pd-text-muted); }
    ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--pd-space-1); }
    .notice { border: 1px solid var(--pd-hairline); border-inline-start: 3px solid var(--pd-border-strong); border-radius: var(--pd-radius-control); padding: var(--pd-space-1) var(--pd-space-2); display: flex; flex-direction: column; gap: var(--pd-space-1); }
    .notice[data-severity='error'], .notice[data-severity='critical'] { border-inline-start-color: var(--pd-danger); }
    .notice[data-severity='warning'] { border-inline-start-color: var(--pd-warning); }
    .notice[data-severity='success'] { border-inline-start-color: var(--pd-success); }
    .notice.focus { outline: 2px solid var(--pd-focus-ring); }
    .row { display: flex; align-items: center; gap: var(--pd-space-1); min-width: 0; }
    .dot { width: 7px; height: 7px; border-radius: 50%; flex: none; }
    .unread .dot { background: var(--pd-accent); }
    .title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: var(--pd-weight-medium); }
    .notice:not(.unread) .title { font-weight: var(--pd-weight-regular); color: var(--pd-text-muted); }
    .time { white-space: nowrap; color: var(--pd-text-muted); font-size: var(--pd-text-caption); font-variant-numeric: tabular-nums; }
    .body { margin: 0; color: var(--pd-text-muted); overflow-wrap: anywhere; user-select: text; }
    .actions { display: flex; flex-wrap: wrap; gap: var(--pd-space-1); justify-content: flex-end; }
    .row-actions { display: flex; gap: 2px; opacity: 0; }
    .notice:hover .row-actions, .notice:focus-within .row-actions { opacity: 1; }
    @media (hover: none) { .row-actions { opacity: 1; } }
    .btn { min-height: 26px; padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; font-size: var(--pd-text-caption); font-weight: var(--pd-weight-medium); cursor: pointer; }
    .btn.ghost { background: transparent; border-color: transparent; color: var(--pd-accent); }
    .btn:disabled { color: var(--pd-text-subtle); cursor: default; }
    .icon-btn { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; border: 0; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text-muted); cursor: pointer; }
    .icon-btn:hover { background: var(--pd-hover); color: var(--pd-text); }
    .btn:focus-visible, .icon-btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  `,
})
export class PdNotificationsPanel {
  protected readonly store = inject(NotificationsStore);
  protected readonly actions = inject(NoticeActions);
  private readonly registry = inject(ContributionRegistry, { optional: true });
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  protected readonly note = signal<string | null>(null);
  protected readonly titleWithCount = titleWithCount;
  protected readonly filters = computed<SuSegment[]>(() => [
    { value: 'all', label: 'All' },
    { value: 'unread', label: 'Unread', count: this.store.unread() },
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
    return kind?.title.text ?? BUILT_IN_SOURCES[source] ?? source;
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

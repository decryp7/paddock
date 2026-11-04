import { afterNextRender, ChangeDetectionStrategy, Component, computed, DestroyRef, effect, ElementRef, inject, Injector, signal, untracked } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatCheckbox } from '@angular/material/checkbox';
import { SuEmptyState, SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { ContributionRegistry } from '@decryp7/paddock-ui/shell';
import { confirm } from '@decryp7/paddock-ui/shell/confirm';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
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
 * all read; messages can be selected (checkboxes, Shift-click for a range) and deleted together, or all at once after a
 * confirmation. The engine keeps the list (dedupe with counts, caps, read state).
 */
@Component({
  selector: 'pd-notifications-panel',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SuEmptyState, SuSegmentedControl, MatCheckbox, MatButton],
  template: `
    <div class="pd-bar" data-testid="notices-filter-bar">
      <pd-segmented-control [label]="t.filter" [options]="filters()" [value]="store.read()" (valueChange)="setRead($event)" />
      <span class="pd-spacer"></span>
      @if (store.all().length) {
        <button type="button" matButton="text" class="pd-quiet pd-nowrap" data-testid="notices-delete-all" (click)="deleteAll()">{{ t.deleteAll }}</button>
      }
      <button type="button" matButton="text" class="pd-quiet pd-nowrap" [disabled]="!store.unread()" data-testid="notices-mark-all" (click)="store.markAllRead()">{{ t.markAllRead }}</button>
    </div>
    @if (store.notices().length) {
      <div class="pd-select-bar">
        <mat-checkbox class="pd-pick" [checked]="allSelected()" [indeterminate]="someSelected()" data-testid="notices-select-all" (change)="toggleAll()">{{ t.selectAll }}</mat-checkbox>
        <span class="pd-spacer"></span>
        @if (selectedIds().length) {
          <button type="button" matButton="outlined" class="pd-nowrap" data-testid="notices-delete-selected" (click)="deleteSelected()">{{ deleteSelectedLabel() }}</button>
        }
      </div>
    }
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
            <mat-checkbox class="pd-pick" [checked]="isSelected(n.id)" [aria-label]="selectLabel(n.title)" [attr.data-testid]="'notice-select-' + n.id" (change)="pick(n.id)" />
            <span class="pd-dot" aria-hidden="true"></span>
            <strong class="pd-title" [attr.title]="titleWithCount(n)">{{ titleWithCount(n) }}</strong>
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
              <button type="button" class="pd-icon-btn" [attr.aria-label]="dismissLabel(n.title)" [attr.data-testid]="'notice-dismiss-' + n.id" (click)="dismissOne(n.id)">
                <svg viewBox="0 0 14 14" width="14" height="14" aria-hidden="true" focusable="false"><path d="M3.5 3.5l7 7M10.5 3.5l-7 7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
              </button>
            </span>
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
    .pd-nowrap { white-space: nowrap; }
    /* The select-all row: its box sits in the items' checkbox column (past the item's border and padding). */
    .pd-select-bar { display: flex; align-items: center; gap: var(--pd-space-1); padding-inline-start: var(--pd-notice-inset); }
    .pd-spacer, .pd-grow { flex: 1; }
    .pd-err { display: flex; align-items: center; gap: var(--pd-space-2); padding: var(--pd-space-1) var(--pd-space-2); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-danger); margin: 0; }
    h3 { margin: var(--pd-space-2) 0 0; font-size: var(--pd-text-caption); font-weight: var(--pd-weight-semibold); color: var(--pd-text-muted); }
    ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--pd-space-1); }
    .pd-notice { position: relative; border: 1px solid var(--pd-hairline); border-inline-start: 3px solid var(--pd-border-strong); border-radius: var(--pd-radius-control); padding: var(--pd-space-1) var(--pd-space-2); display: grid; grid-template-columns: var(--pd-pick-size) var(--pd-dot-size) minmax(0, 1fr) auto; align-items: start; column-gap: var(--pd-space-1); row-gap: var(--pd-space-1); }
    .pd-notice[data-severity='error'], .pd-notice[data-severity='critical'] { border-inline-start-color: var(--pd-danger); }
    .pd-notice[data-severity='warning'] { border-inline-start-color: var(--pd-warning); }
    .pd-notice[data-severity='success'] { border-inline-start-color: var(--pd-success); }
    .pd-notice.pd-focus { outline: 2px solid var(--pd-focus-ring); }
    /* One grid: [pick][dot][title][time], the body and buttons under the title (column 3 to the end). The pick column is the touch target
       (density-aware); the shared rule in _material.scss pulls the box to the cell's left edge. The first line's height is one value, so the
       box, the dot and the time pin to it however many lines the title wraps to. */
    :host { --pd-pick-size: var(--pd-control-height); --pd-dot-size: 7px; --pd-notice-line: calc(var(--pd-text-body) * 1.4); --pd-notice-inset: calc(3px + var(--pd-space-2)); }
    .pd-pick { --mat-checkbox-state-layer-size: var(--pd-pick-size); grid-column: 1; margin-block: calc((var(--pd-notice-line) - var(--pd-pick-size)) / 2); }
    .pd-select-bar .pd-pick { margin-block: 0; }
    .pd-dot { grid-column: 2; width: var(--pd-dot-size); height: var(--pd-dot-size); margin-block: calc((var(--pd-notice-line) - var(--pd-dot-size)) / 2); border-radius: 50%; }
    .pd-unread .pd-dot { background: var(--pd-accent); }
    /* Wraps (two lines, then an ellipsis with the full text in the tooltip): a one-line ellipsis cut titles with room to spare. */
    .pd-title { grid-column: 3; line-height: var(--pd-notice-line); min-width: 0; display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; overflow-wrap: anywhere; font-weight: var(--pd-weight-medium); }
    .pd-notice:not(.pd-unread) .pd-title { font-weight: var(--pd-weight-regular); color: var(--pd-text-muted); }
    .pd-time { grid-column: 4; line-height: var(--pd-notice-line); white-space: nowrap; color: var(--pd-text-muted); font-size: var(--pd-text-caption); font-variant-numeric: tabular-nums; }
    .pd-body { grid-column: 3 / -1; margin: 0; color: var(--pd-text-muted); overflow-wrap: anywhere; -webkit-user-select: text; user-select: text; }
    .pd-actions { grid-column: 3 / -1; display: flex; flex-wrap: wrap; gap: var(--pd-space-1); justify-content: flex-end; }
    .pd-row-actions { position: absolute; inset-block-start: 2px; inset-inline-end: var(--pd-space-1); display: flex; gap: 2px; padding-inline-start: var(--pd-space-2); background: var(--pd-surface-3); border-radius: var(--pd-radius-control); opacity: 0; }
    /* Shown by hover or by focus on one of its own buttons, never by focus on a body action (that would cover the time). */
    .pd-notice:hover .pd-row-actions, .pd-row-actions:focus-within { opacity: 1; }
    /* The buttons sit over the time, so it gives way whole while they show (not cut in half). */
    .pd-notice:hover .pd-time, .pd-notice:has(.pd-row-actions:focus-within) .pd-time { visibility: hidden; }
    @media (hover: none) { .pd-row-actions { position: static; grid-column: 3 / -1; justify-self: end; opacity: 1; padding-inline-start: 0; } .pd-notice:hover .pd-time { visibility: visible; } }
    /* Delete all and Mark all read are text buttons in the muted colour; hover fills and reads in the normal text colour (Delete all's danger is in its confirmation). */
    .pd-quiet { --mat-button-text-label-text-color: var(--pd-text-muted); --mat-button-text-state-layer-color: var(--pd-text); }
    .pd-quiet:hover:not(:disabled) { --mat-button-text-label-text-color: var(--pd-text); }
    .pd-btn { min-height: 26px; padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; font-size: var(--pd-text-caption); font-weight: var(--pd-weight-medium); cursor: pointer; }
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
    selectAll: $localize`:@@pd.notifications.select-all:Select all notifications`,
    deleteAll: $localize`:@@pd.notifications.delete-all:Delete all`,
    unreadMark: $localize`:@@pd.notifications.unread-mark:, unread`,
  };
  protected readonly store = inject(NotificationsStore);
  protected readonly actions = inject(NoticeActions);
  private readonly registry = inject(ContributionRegistry, { optional: true });
  protected readonly dialogs = inject(DialogService);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly chosen = signal<ReadonlySet<number>>(new Set());
  private anchor: number | null = null;
  private shift = false;
  private keyShift = false;
  private readonly injector = inject(Injector);
  /** The listed messages' ids in the order shown (group by group). */
  private readonly shownIds = computed(() => this.store.groups().flatMap((g) => g.notices.map((n) => n.id)));
  /** The selection that is still listed (a message that went away is no longer selected). */
  protected readonly selectedIds = computed(() => this.shownIds().filter((id) => this.chosen().has(id)));
  protected readonly allSelected = computed(() => this.shownIds().length > 0 && this.selectedIds().length === this.shownIds().length);
  protected readonly someSelected = computed(() => this.selectedIds().length > 0 && !this.allSelected());
  protected readonly note = signal<string | null>(null);
  protected readonly titleWithCount = titleWithCount;
  protected readonly filters = computed<SuSegment[]>(() => [
    { value: 'all', label: $localize`:@@pd.notifications.filter.all:All` },
    { value: 'unread', label: $localize`:@@pd.notifications.filter.unread:Unread`, count: this.store.unread() },
  ]);

  constructor() {
    // Whether Shift was held for the click that changes a checkbox (the checkbox reports its change before the click bubbles).
    const el = this.host.nativeElement;
    // A key press on a checkbox (Shift+Space) arrives as a click without modifiers, so the key state is kept too.
    const key = (e: Event): void => {
      this.keyShift = (e as KeyboardEvent).shiftKey === true;
    };
    const remember = (e: Event): void => {
      const m = e as MouseEvent;
      this.shift = m.shiftKey === true || (m.detail === 0 && this.keyShift);
    };
    el.addEventListener('click', remember, true);
    el.addEventListener('keydown', key, true);
    el.addEventListener('keyup', key, true);
    inject(DestroyRef).onDestroy(() => {
      el.removeEventListener('click', remember, true);
      el.removeEventListener('keydown', key, true);
      el.removeEventListener('keyup', key, true);
    });
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
    return kind?.title.text ?? BUILT_IN_SOURCES[source]?.() ?? source.charAt(0).toUpperCase() + source.slice(1);
  }

  protected markReadLabel(title: string): string {
    return $localize`:@@pd.notifications.mark-read:Mark read: ${title}:title:`;
  }

  protected dismissLabel(title: string): string {
    return $localize`:@@pd.notifications.dismiss:Dismiss ${title}:title:`;
  }

  protected isSelected(id: number): boolean {
    return this.chosen().has(id);
  }

  protected selectLabel(title: string): string {
    return $localize`:@@pd.notifications.select:Select ${title}:title:`;
  }

  protected deleteSelectedLabel(): string {
    return $localize`:@@pd.notifications.delete-selected:Delete selected (${this.selectedIds().length}:count:)`;
  }

  /** Toggles one message; with Shift held, the range from the last one picked. */
  protected pick(id: number): void {
    const next = new Set(this.chosen());
    const on = !next.has(id);
    const order = this.shownIds();
    const from = this.shift && this.anchor !== null ? order.indexOf(this.anchor) : -1;
    const to = order.indexOf(id);
    const ids = from >= 0 && to >= 0 ? order.slice(Math.min(from, to), Math.max(from, to) + 1) : [id];
    for (const i of ids) {
      if (on) next.add(i);
      else next.delete(i);
    }
    this.anchor = id;
    this.shift = false;
    this.chosen.set(next);
  }

  protected toggleAll(): void {
    this.chosen.set(this.allSelected() ? new Set() : new Set(this.shownIds()));
    this.anchor = null;
  }

  protected async deleteSelected(): Promise<void> {
    const ids = this.selectedIds();
    this.chosen.set(new Set());
    this.anchor = null;
    const target = this.focusTargetAfter(ids);
    await this.store.dismissMany(ids);
    this.focusAfterRender(target);
  }

  /** Dismisses one message and keeps the focus in the list. */
  protected async dismissOne(id: number): Promise<void> {
    const target = this.focusTargetAfter([id]);
    await this.store.dismiss(id);
    this.focusAfterRender(target);
  }

  /** The message that should take the focus when `removed` go: the next one still listed, else the one before. */
  private focusTargetAfter(removed: readonly number[]): number | null {
    const order = this.shownIds();
    const gone = new Set(removed);
    const first = order.findIndex((id) => gone.has(id));
    if (first < 0) return null;
    return order.slice(first).find((id) => !gone.has(id)) ?? [...order.slice(0, first)].reverse().find((id) => !gone.has(id)) ?? null;
  }

  /** Once the list is drawn again: the target's checkbox, else the filter bar (never the page body). */
  private focusAfterRender(target: number | null): void {
    afterNextRender(
      () => {
        const root = this.host.nativeElement;
        const el =
          (target !== null ? root.querySelector<HTMLElement>(`[data-testid="notice-select-${target}"] input`) : null) ??
          root.querySelector<HTMLElement>('[data-testid="notices-filter-bar"] input, [data-testid="notices-filter-bar"] button');
        el?.focus();
      },
      { injector: this.injector },
    );
  }

  /**
   * Asks first (it cannot be undone), then removes the messages the user saw when asking; one that arrives while the
   * question is open stays. The question's count comes from the same list.
   */
  protected async deleteAll(): Promise<void> {
    const seen = this.store.all().map((n) => n.id);
    const count = seen.length;
    const yes = await confirm(this.dialogs, {
      heading: $localize`:@@pd.notifications.delete-all.heading:Delete all notifications?`,
      message:
        count === 1
          ? $localize`:@@pd.notifications.delete-all.message-one:The notification is removed from the centre. This cannot be undone.`
          : $localize`:@@pd.notifications.delete-all.message:${count}:count: notifications are removed from the centre. This cannot be undone.`,
      confirm: { label: $localize`:@@pd.notifications.delete-all.confirm:Delete all`, destructive: true },
    });
    if (!yes) return;
    this.chosen.set(new Set());
    this.anchor = null;
    await this.store.dismissAll(seen);
    this.focusAfterRender(null);
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

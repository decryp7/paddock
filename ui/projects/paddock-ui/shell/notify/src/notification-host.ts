import { ChangeDetectionStrategy, Component, computed, DestroyRef, effect, inject, signal } from '@angular/core';
import { ToastArea } from '@decryp7/paddock-ui/shell';
import { errorMessage } from '@decryp7/paddock-ui/shell/store';
import { MAX_VISIBLE, NOTIFY_CLOCK, TOAST_MS } from './notify-config';
import { NOTIFICATION_SOURCES, mergeNotifications, persists, type HostAction, type HostNotification } from './notification-host.model';

/** How often the host checks its toasts' time. */
const TICK_MS = 250;

interface Timer {
  /** The occurrence the timer belongs to. */
  key: string;
  /** When it goes, or null while the pointer or focus holds it. */
  deadline: number | null;
  ttl: number;
}

/**
 * The one place toasts appear in a window (ADR 0010, Debrief ADR 0055). One toast per cause with its count, technical
 * details folded behind "Details", updated in place; errors and critical failures stay until resolved or dismissed; a
 * cause the status chip states (`health:...`) is never toasted. The host owns the time of every toast (sources only
 * post, update and resolve): a toast that times out stays `ttl`, and starts over when its cause is posted again. Mounted
 * once per window by `provideShellNotify` (or placed in the frame).
 *
 * Nothing moves under the pointer. The stack is anchored at the bottom of the window, so the newest toast enters at the top
 * (the DOM lists the newest first, as they appear) and the ones already there keep their place; a cause posted again is
 * updated in place. While the pointer or focus is anywhere in the stack, its time is held and its set of toasts is frozen: a
 * new toast waits, nothing is dropped past `MAX_VISIBLE`, and the order stays; only what goes away (dismissed, resolved)
 * leaves at once. A press is two events, and a layout change between them would send the click to another element.
 */
@Component({
  selector: 'pd-notification-host',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="pd-stack" data-testid="notification-stack" role="region" [attr.aria-label]="t.toasts" aria-live="polite" (mouseenter)="enter()" (focusin)="enter()" (mouseleave)="leave($event)" (focusout)="leave($event)">
      @for (n of items(); track n.cause) {
        <div
          class="pd-toast"
          data-testid="notification"
          [attr.data-severity]="n.severity"
          [attr.data-cause]="n.cause"
          [attr.role]="persistent(n) ? 'alert' : 'status'"
        >
          <div class="pd-head">
            <span class="pd-mark" aria-hidden="true"></span>
            <strong class="pd-title" [class.pd-has-short]="!!n.short"><span class="pd-full">{{ n.title }}</span>@if (n.short) {<span class="pd-short" data-testid="notification-short">{{ n.short }}</span>}@if ((n.count ?? 1) > 1) {<span class="pd-count" data-testid="notification-count"> (×{{ n.count }})</span>}</strong>
          </div>
          @if (n.body) {
            <div class="pd-body">{{ n.body }}</div>
          }
          @if (n.actions?.length || n.details) {
            <div class="pd-actions">
              @for (a of n.actions; track $index) {
                <button type="button" class="pd-btn" [class.pd-primary]="a.kind === 'primary'" [class.pd-ghost]="a.kind === 'ghost'" [attr.data-testid]="'notification-action-' + (a.id ?? $index)" (click)="run(n, a)">{{ a.label }}</button>
              }
              @if (n.details) {
                <button type="button" class="pd-btn pd-ghost" [attr.aria-expanded]="isOpen(n)" data-testid="notification-details-toggle" (click)="toggle(n)">{{ t.details }}</button>
              }
            </div>
          }
          @if (n.details && isOpen(n)) {
            <code class="pd-details" data-testid="notification-details">{{ n.details }}</code>
          }
          @if (failed()[n.cause]; as why) {
            <div class="pd-body pd-err" role="alert">{{ why }}</div>
          }
          <button type="button" class="pd-x" [attr.aria-label]="t.dismissToast" data-testid="notification-dismiss" (click)="n.dismiss()">
            <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true" focusable="false"><path d="M2.5 2.5l7 7M9.5 2.5l-7 7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
          </button>
        </div>
      }
    </div>
  `,
  host: { '[style.--pd-toast-lift]': "area.lift() ? 'calc(' + area.lift() + 'px + var(--pd-space-2))' : '0px'" },
  styles: `
    :host { position: fixed; right: var(--pd-space-4); bottom: calc(var(--pd-statusbar-height, 24px) + var(--pd-page-footer-safe, 56px) + var(--pd-space-2)); z-index: var(--pd-z-toast); pointer-events: none; margin-bottom: var(--pd-toast-lift, 0px); }
    .pd-stack { pointer-events: auto; display: flex; flex-direction: column; gap: var(--pd-space-2); align-items: flex-end; }
    .pd-toast { pointer-events: auto; position: relative; width: 360px; max-width: calc(100vw - 2 * var(--pd-space-4)); box-sizing: border-box; padding: var(--pd-space-2) var(--pd-space-8) var(--pd-space-2) var(--pd-space-3); border: 1px solid var(--pd-hairline); border-inline-start: 3px solid var(--pd-info); border-radius: var(--pd-radius-popover); background: var(--pd-surface-3); color: var(--pd-text); box-shadow: var(--pd-shadow-2); display: flex; flex-direction: column; gap: var(--pd-space-1); font-size: var(--pd-text-body); }
    .pd-toast[data-severity='success'] { border-inline-start-color: var(--pd-success); }
    .pd-toast[data-severity='warning'] { border-inline-start-color: var(--pd-warning); }
    .pd-toast[data-severity='error'], .pd-toast[data-severity='critical'] { border-inline-start-color: var(--pd-danger); }
    .pd-head { display: flex; align-items: center; gap: var(--pd-space-2); min-width: 0; }
    .pd-mark { flex: none; width: 8px; height: 8px; border-radius: 50%; background: var(--pd-info); }
    [data-severity='success'] .pd-mark { background: var(--pd-success); }
    [data-severity='warning'] .pd-mark { background: var(--pd-warning); border-radius: 1px; transform: rotate(45deg); }
    [data-severity='error'] .pd-mark, [data-severity='critical'] .pd-mark { background: var(--pd-danger); border-radius: 1px; }
    .pd-title { overflow-wrap: anywhere; font-weight: var(--pd-weight-semibold); }
    .pd-count { color: var(--pd-text-muted); font-weight: var(--pd-weight-regular); font-size: var(--pd-text-caption); }
    .pd-body { color: var(--pd-text-muted); line-height: var(--pd-lh-body); overflow-wrap: anywhere; -webkit-user-select: text; user-select: text; }
    .pd-err { color: var(--pd-danger); }
    .pd-actions { display: flex; gap: var(--pd-space-1); flex-wrap: wrap; }
    .pd-btn { min-height: 26px; padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; font-size: var(--pd-text-caption); font-weight: var(--pd-weight-medium); cursor: pointer; }
    .pd-btn.pd-primary { background: var(--pd-accent-fill); border-color: transparent; color: var(--pd-on-accent); }
    .pd-btn.pd-ghost { background: transparent; border-color: transparent; color: var(--pd-accent); }
    .pd-btn:focus-visible, .pd-x:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .pd-details { display: block; max-height: 160px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; color: var(--pd-text-muted); font-family: var(--pd-font-mono); font-size: var(--pd-text-caption); -webkit-user-select: text; user-select: text; }
    .pd-x { position: absolute; top: var(--pd-space-1); right: var(--pd-space-1); display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; border: 0; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text-muted); cursor: pointer; }
    .pd-x:hover { background: var(--pd-hover); color: var(--pd-text); }
    /* A small window (review 2, P3-14): one line above the status bar, full width, never wider than 360 px: the message, the
       first action and the close. The body, details and further actions stay in the notification centre; the two newest
       toasts show, so the stack never covers a fifth of the window. */
    .pd-short { display: none; }
    @media (max-width: 480px) {
      .pd-has-short .pd-full { display: none; }
      .pd-has-short .pd-short { display: inline; }
      :host { left: var(--pd-space-2); right: var(--pd-space-2); }
      .pd-stack { align-items: stretch; }
      .pd-toast { width: auto; max-width: 360px; align-self: flex-end; flex-direction: row; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2); padding: var(--pd-space-1) var(--pd-space-1) var(--pd-space-1) var(--pd-space-2); }
      .pd-toast:nth-child(n + 3) { display: none; }
      .pd-head { flex: 1 1 0; }
      .pd-title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; overflow-wrap: normal; }
      .pd-body:not(.pd-err), .pd-details, .pd-actions .pd-btn:not(:first-child) { display: none; }
      .pd-err { flex: 1 1 100%; }
      .pd-actions { flex: none; flex-wrap: nowrap; }
      .pd-x { position: static; flex: none; }
    }
  `,
})
export class PdNotificationHost {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    toasts: $localize`:@@pd.notifications.toasts:Notifications`,
    dismissToast: $localize`:@@pd.notifications.dismiss-toast:Dismiss notification`,
    details: $localize`:@@pd.notifications.details:Details`,
  };
  /** The undo snack bar's height, when one is open: this host stacks above it. */
  protected readonly area = inject(ToastArea);
  private readonly sources = inject(NOTIFICATION_SOURCES, { optional: true }) ?? [];
  private readonly clock = inject(NOTIFY_CLOCK);
  /** Every toast, oldest first (not yet cut to `MAX_VISIBLE`). */
  private readonly merged = computed(() => mergeNotifications(this.sources.flatMap((s) => s.items())));
  /** The causes on screen when the pointer or focus came into the stack, oldest first; null while it is outside. */
  private readonly frozen = signal<readonly string[] | null>(null);
  /** What is shown, newest first: the live set, or while frozen the frozen causes that are still posted (updated in place). */
  protected readonly items = computed(() => {
    const merged = this.merged();
    const frozen = this.frozen();
    const kept = frozen === null ? [] : frozen.map((c) => merged.find((n) => n.cause === c)).filter((n): n is HostNotification => !!n);
    // A stack that emptied under the pointer has nothing left to protect: what arrives next shows.
    return (kept.length ? kept : merged.slice(-MAX_VISIBLE)).slice().reverse();
  });
  private readonly timed = computed(() => this.items().some((n) => !persists(n)));
  private readonly opened = signal<ReadonlySet<string>>(new Set());
  /** An action that failed: why, until the next try. */
  protected readonly failed = signal<Readonly<Record<string, string>>>({});
  /** Per cause: the occurrence its timer belongs to and when it goes. */
  private readonly timers = new Map<string, Timer>();
  /** The pointer or focus is in the stack: no toast's time runs. */
  private held = false;

  constructor() {
    // The toasts' time is outside the signal graph: a timer per toast that times out, started when it appears or is
    // posted again (a new stamp).
    effect(() => this.sync(this.items()));
    // The clock ticks only while a toast that times out is shown.
    effect((onCleanup) => {
      if (this.timed()) onCleanup(this.clock.every(TICK_MS, () => this.tick()));
    });
    inject(DestroyRef).onDestroy(() => this.timers.clear());
  }

  protected persistent(n: HostNotification): boolean {
    return persists(n);
  }

  private sync(items: readonly HostNotification[]): void {
    const live = new Set(items.map((n) => n.cause));
    for (const cause of [...this.timers.keys()]) if (!live.has(cause)) this.timers.delete(cause);
    const now = this.clock.now();
    for (const n of items) {
      if (persists(n)) {
        this.timers.delete(n.cause);
        continue;
      }
      const key = `${n.instance ?? ''}#${n.stamp ?? n.count ?? ''}`;
      const current = this.timers.get(n.cause);
      if (current?.key === key) continue;
      const ttl = n.ttl ?? TOAST_MS;
      this.timers.set(n.cause, { key, ttl, deadline: this.held || (current && current.deadline === null) ? null : now + ttl });
    }
  }

  private tick(): void {
    const now = this.clock.now();
    for (const n of this.items()) {
      const t = this.timers.get(n.cause);
      if (t && t.deadline !== null && now >= t.deadline) {
        this.timers.delete(n.cause);
        n.dismiss();
      }
    }
  }

  /** The pointer or focus came into the stack: the time of every toast stops and the set of toasts is frozen. */
  protected enter(): void {
    if (this.held) return;
    this.held = true;
    this.frozen.set(this.items().map((n) => n.cause).reverse());
    for (const t of this.timers.values()) t.deadline = null;
  }

  /** The pointer or focus left the stack: once neither is in it, the time of every toast starts over and the waiting ones appear. */
  protected leave(e: MouseEvent | FocusEvent): void {
    const stack = e.currentTarget as HTMLElement;
    const focusStays = e instanceof FocusEvent ? stack.contains(e.relatedTarget as Node | null) : stack.contains(document.activeElement);
    const pointerStays = e instanceof FocusEvent && stack.matches(':hover');
    if (focusStays || pointerStays || !this.held) return;
    this.held = false;
    this.frozen.set(null);
    const now = this.clock.now();
    this.sync(this.items());
    for (const t of this.timers.values()) t.deadline = now + t.ttl;
  }

  private key(n: HostNotification): string {
    return `${n.cause}#${n.instance ?? ''}`;
  }

  protected isOpen(n: HostNotification): boolean {
    return this.opened().has(this.key(n));
  }

  protected toggle(n: HostNotification): void {
    const key = this.key(n);
    this.opened.update((s) => {
      const next = new Set(s);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  }

  /** Runs an action; the toast stays when it failed, with the reason. */
  protected async run(n: HostNotification, a: HostAction): Promise<void> {
    this.failed.update((f) => ({ ...f, [n.cause]: '' }));
    try {
      await a.run();
    } catch (e) {
      this.failed.update((f) => ({ ...f, [n.cause]: errorMessage(e, $localize`:@@pd.notifications.action-failed:The action failed.`) }));
    }
  }
}
